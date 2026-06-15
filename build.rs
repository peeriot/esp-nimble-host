use std::fmt::Write as _;
use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};

use flate2::read::GzDecoder;
use serde::Deserialize;
use tar::Archive;
use walkdir::WalkDir;

/// NimBLE version to download (must match the tag on GitHub).
const NIMBLE_VERSION: &str = "nimble_1_9_0_tag";

/// URL template for the Apache NimBLE source tarball.
const NIMBLE_DOWNLOAD_URL: &str =
    "https://github.com/apache/mynewt-nimble/archive/refs/tags/nimble_1_9_0_tag.tar.gz";

/// Directory name inside the extracted tarball.
const NIMBLE_EXTRACTED_DIR: &str = "mynewt-nimble-nimble_1_9_0_tag";

/// Download and extract the NimBLE source into `out_dir` if not already present.
/// Returns the path to the extracted NimBLE root.
fn ensure_nimble_source(out_dir: &Path) -> PathBuf {
    let nimble_dir = out_dir.join(NIMBLE_EXTRACTED_DIR);

    if nimble_dir.exists() {
        // Already downloaded and extracted in a previous build.
        return nimble_dir;
    }

    println!(
        "cargo:warning=Downloading Apache NimBLE ({}) from GitHub...",
        NIMBLE_VERSION,
    );

    let response = ureq::get(NIMBLE_DOWNLOAD_URL).call().unwrap_or_else(|e| {
        panic!("Failed to download NimBLE source from {NIMBLE_DOWNLOAD_URL}: {e}");
    });

    let body = response
        .into_body()
        .read_to_vec()
        .expect("Failed to read NimBLE tarball");

    let tar = GzDecoder::new(Cursor::new(body));
    let mut archive = Archive::new(tar);
    archive
        .unpack(out_dir)
        .expect("Failed to extract NimBLE tarball");

    assert!(
        nimble_dir.exists(),
        "Expected directory '{}' not found after extraction",
        nimble_dir.display()
    );

    nimble_dir
}

/// Patch NimBLE's `os_memblock_get` to zero returned memory pool blocks.
///
/// # Why this is needed
///
/// NimBLE's memory pool (`os_mempool`) reuses freed blocks by threading a
/// free-list pointer through the first `sizeof(void *)` bytes of each block.
/// When a block is re-allocated via `os_memblock_get`, those bytes still
/// contain the stale free-list pointer — the pool never zeroes them.
///
/// The esp-radio BLE NPL (NimBLE Porting Layer) stores a heap-allocated
/// `Event` pointer inside `ble_npl_event.dummy` (the only field — 4 bytes).
/// `ble_npl_event_init` skips allocation when `dummy != 0`, assuming the
/// event is already initialised. A recycled pool block therefore looks
/// "already initialised" and the stale free-list pointer is later
/// dereferenced as a function pointer → **Illegal Instruction** crash.
///
/// Zeroing each block on allocation makes `dummy == 0` so that
/// `ble_npl_event_init` allocates a fresh `Event` every time, which is the
/// correct behaviour. The performance cost is negligible (blocks are tiny,
/// typically 4–16 bytes).
fn patch_os_mempool_zero_on_alloc(nimble_dir: &Path) {
    let file = nimble_dir.join("porting/nimble/src/os_mempool.c");
    let src = fs::read_to_string(&file).expect("Failed to read os_mempool.c");

    // The original code in os_memblock_get:
    //     if (block) {
    //         os_mempool_poison_check(mp, block);
    //         os_mempool_guard_check(mp, block);
    //     }
    //
    // We add a memset right after the guard checks, before the block is
    // returned to the caller.
    let needle = "        if (block) {\n            os_mempool_poison_check(mp, block);\n            os_mempool_guard_check(mp, block);\n        }";

    let replacement = "        if (block) {\n            os_mempool_poison_check(mp, block);\n            os_mempool_guard_check(mp, block);\n            /* [esp-nimble-host patch] Zero block so that ble_npl_event.dummy\n             * is 0 after re-allocation from the pool.  See build.rs for the\n             * full rationale (stale free-list pointers vs. ble_npl_event_init). */\n            memset(block, 0, mp->mp_block_size);\n        }";

    if !src.contains(needle) {
        if src.contains("[esp-nimble-host patch]") {
            // Already patched in a previous build.
            return;
        }
        panic!(
            "Could not find the expected code pattern in os_mempool.c to apply \
             the zero-on-alloc patch. The NimBLE version may have changed."
        );
    }

    let patched = src.replace(needle, replacement);
    fs::write(&file, patched).expect("Failed to write patched os_mempool.c");
}

/// Patch NimBLE's `ble_hs_event_rx_hci_ev` to call `ble_npl_event_deinit`
/// before returning the event block to the memory pool.
///
/// # Why this is needed
///
/// When a `ble_npl_event` block is returned to the pool via `os_memblock_put`,
/// the pool overwrites the first 4 bytes (`dummy`) with its free-list pointer.
/// The heap-allocated `Event` struct that `dummy` previously pointed to becomes
/// unreachable and is **leaked** (12 bytes per cycle).
///
/// By calling `ble_npl_event_deinit(ev)` before `os_memblock_put`, we free the
/// heap `Event` and set `dummy = 0`.  The zero-on-alloc patch in
/// `os_memblock_get` (see [`patch_os_mempool_zero_on_alloc`]) then acts as a
/// safety net: even if a return-to-pool site is missed, the stale free-list
/// pointer won't be mistaken for a valid `Event` — it will be zeroed and
/// `ble_npl_event_init` will allocate a fresh `Event`.
///
/// # Maintenance note
///
/// As of NimBLE 1.9, `ble_hs_event_rx_hci_ev` in `ble_hs.c` is the **only**
/// function that returns `ble_npl_event` blocks to `ble_hs_hci_ev_pool`.
/// If a future NimBLE version adds more `os_memblock_put` call sites for this
/// pool, those sites must also be patched — otherwise they will leak `Event`
/// structs.  The zero-on-alloc safety net prevents crashes but not leaks.
///
/// To audit: `grep -rn 'os_memblock_put.*ble_hs_hci_ev_pool' nimble/host/src/`
fn patch_ble_hs_event_deinit_before_pool_put(nimble_dir: &Path) {
    let file = nimble_dir.join("nimble/host/src/ble_hs.c");
    let src = fs::read_to_string(&file).expect("Failed to read ble_hs.c");

    let needle = "    rc = os_memblock_put(&ble_hs_hci_ev_pool, ev);";

    let replacement = "    /* [esp-nimble-host patch] Free the heap-allocated Event struct before\n     * returning the block to the pool.  Without this, the Event is leaked\n     * every time the pool recycles a block.  See build.rs for details. */\n    { extern void ble_npl_event_deinit(struct ble_npl_event *ev); ble_npl_event_deinit(ev); }\n    rc = os_memblock_put(&ble_hs_hci_ev_pool, ev);";

    if !src.contains(needle) {
        if src.contains("ble_npl_event_deinit(ev)") {
            // Already patched in a previous build.
            return;
        }
        panic!(
            "Could not find the expected code pattern in ble_hs.c to apply \
             the event-deinit-before-pool-put patch. The NimBLE version may have changed."
        );
    }

    let patched = src.replace(needle, replacement);
    fs::write(&file, patched).expect("Failed to write patched ble_hs.c");
}

// ── nimble-config.toml schema ─────────────────────────────────────────────────

#[derive(Deserialize, Default)]
#[serde(default)]
struct NimbleConfig {
    roles: RolesConfig,
    connections: ConnectionsConfig,
    transport: TransportConfig,
    msys: MsysConfig,
    gatt: GattConfig,
    l2cap: L2capConfig,
    storage: StorageConfig,
    security: SecurityConfig,
}

#[derive(Deserialize)]
#[serde(default)]
struct RolesConfig {
    central: bool,
    observer: bool,
    peripheral: bool,
    broadcaster: bool,
}

impl Default for RolesConfig {
    fn default() -> Self {
        Self {
            central: true,
            observer: true,
            peripheral: false,
            broadcaster: false,
        }
    }
}

#[derive(Deserialize)]
#[serde(default)]
struct ConnectionsConfig {
    max_connections: u16,
}

impl Default for ConnectionsConfig {
    fn default() -> Self {
        Self { max_connections: 1 }
    }
}

#[derive(Deserialize)]
#[serde(default)]
struct TransportConfig {
    acl_count: u16,
    acl_size: u16,
    evt_count: u16,
    evt_discardable_count: u16,
    evt_size: u16,
}

impl Default for TransportConfig {
    fn default() -> Self {
        Self {
            acl_count: 6,
            acl_size: 251,
            evt_count: 4,
            evt_discardable_count: 8,
            evt_size: 70,
        }
    }
}

#[derive(Deserialize)]
#[serde(default)]
struct MsysConfig {
    block_count: u16,
    block_size: u16,
}

impl Default for MsysConfig {
    fn default() -> Self {
        Self {
            block_count: 8,
            block_size: 292,
        }
    }
}

#[derive(Deserialize)]
#[serde(default)]
struct GattConfig {
    preferred_mtu: u16,
    max_procs: u16,
    max_prep_entries: u16,
    resume_rate_ms: u16,
}

impl Default for GattConfig {
    fn default() -> Self {
        Self {
            preferred_mtu: 128,
            max_procs: 4,
            max_prep_entries: 0,
            resume_rate_ms: 1000,
        }
    }
}

#[derive(Deserialize)]
#[serde(default)]
struct L2capConfig {
    max_channels: u16,
    max_sig_procs: u16,
}

impl Default for L2capConfig {
    fn default() -> Self {
        Self {
            max_channels: 0,
            max_sig_procs: 1,
        }
    }
}

#[derive(Deserialize)]
#[serde(default)]
struct StorageConfig {
    max_bonds: u16,
    max_cccds: u16,
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            max_bonds: 3,
            max_cccds: 8,
        }
    }
}

#[derive(Deserialize)]
#[serde(default)]
struct SecurityConfig {
    legacy: bool,
    sc: bool,
    mitm: bool,
    bonding: bool,
    max_procs: u16,
}

impl Default for SecurityConfig {
    fn default() -> Self {
        Self {
            legacy: false,
            sc: false,
            mitm: false,
            bonding: false,
            max_procs: 1,
        }
    }
}

/// Load nimble-config.toml from the consuming project, or fall back to defaults.
///
/// Search order:
///   1. `NIMBLE_CONFIG_DIR` env var — set this in `.cargo/config.toml` of the
///      consuming crate for full control over the path (e.g. in a deep workspace).
///   2. Workspace root (`CARGO_WORKSPACE_DIR`) — works automatically when the
///      consuming crate places `nimble-config.toml` at its workspace root.
///   3. `CARGO_MANIFEST_DIR` — this crate's own root; used when building the
///      library directly (e.g. `cargo clippy` in the repo) where
///      `CARGO_WORKSPACE_DIR` may not be set.
///   4. Built-in defaults — when none of the above yields a file.
fn load_config() -> NimbleConfig {
    // Tell cargo to re-run if the override var changes.
    println!("cargo:rerun-if-env-changed=NIMBLE_CONFIG_DIR");

    let search_paths: Vec<PathBuf> = [
        std::env::var("NIMBLE_CONFIG_DIR").ok().map(PathBuf::from),
        std::env::var("CARGO_WORKSPACE_DIR").ok().map(PathBuf::from),
        // CARGO_MANIFEST_DIR points to this crate's root — useful when building
        // the library directly (e.g. `cargo clippy` in the repo) where
        // CARGO_WORKSPACE_DIR may not be set.
        std::env::var("CARGO_MANIFEST_DIR").ok().map(PathBuf::from),
    ]
    .into_iter()
    .flatten()
    .collect();

    for dir in &search_paths {
        let config_path = dir.join("nimble-config.toml");
        if config_path.exists() {
            println!("cargo:rerun-if-changed={}", config_path.display());
            println!(
                "cargo:warning=Using NimBLE config from: {}",
                config_path.display()
            );
            let content = fs::read_to_string(&config_path)
                .unwrap_or_else(|e| panic!("Failed to read {}: {e}", config_path.display()));
            return toml::from_str(&content)
                .unwrap_or_else(|e| panic!("Failed to parse {}: {e}", config_path.display()));
        }
    }

    println!("cargo:warning=No nimble-config.toml found, using built-in defaults");
    NimbleConfig::default()
}

/// Generate a C header that overrides NimBLE's syscfg defaults.
///
/// The generated file defines `MYNEWT_VAL_*` macros BEFORE the stock
/// `syscfg.h` is included. Because `syscfg.h` uses `#ifndef` guards,
/// our definitions take precedence.
fn generate_syscfg_override(config: &NimbleConfig, out_dir: &Path) -> PathBuf {
    let mut h = String::new();

    writeln!(
        h,
        "/* Generated by esp-nimble-host build.rs from nimble-config.toml */"
    )
    .unwrap();
    writeln!(h, "#ifndef NIMBLE_CONFIG_OVERRIDE_H").unwrap();
    writeln!(h, "#define NIMBLE_CONFIG_OVERRIDE_H").unwrap();
    writeln!(h).unwrap();

    // Roles
    writeln!(h, "/* Roles */").unwrap();
    writeln!(
        h,
        "#define MYNEWT_VAL_BLE_ROLE_CENTRAL ({})",
        config.roles.central as u8
    )
    .unwrap();
    writeln!(
        h,
        "#define MYNEWT_VAL_BLE_ROLE_OBSERVER ({})",
        config.roles.observer as u8
    )
    .unwrap();
    writeln!(
        h,
        "#define MYNEWT_VAL_BLE_ROLE_PERIPHERAL ({})",
        config.roles.peripheral as u8
    )
    .unwrap();
    writeln!(
        h,
        "#define MYNEWT_VAL_BLE_ROLE_BROADCASTER ({})",
        config.roles.broadcaster as u8
    )
    .unwrap();
    writeln!(h).unwrap();

    // Connections
    writeln!(h, "/* Connections */").unwrap();
    writeln!(
        h,
        "#define MYNEWT_VAL_BLE_MAX_CONNECTIONS ({})",
        config.connections.max_connections
    )
    .unwrap();
    writeln!(h).unwrap();

    // Transport
    writeln!(h, "/* Transport buffers */").unwrap();
    writeln!(
        h,
        "#define MYNEWT_VAL_BLE_TRANSPORT_ACL_COUNT ({})",
        config.transport.acl_count
    )
    .unwrap();
    writeln!(
        h,
        "#define MYNEWT_VAL_BLE_TRANSPORT_ACL_FROM_HS_COUNT ({})",
        config.transport.acl_count
    )
    .unwrap();
    writeln!(
        h,
        "#define MYNEWT_VAL_BLE_TRANSPORT_ACL_FROM_LL_COUNT ({})",
        config.transport.acl_count
    )
    .unwrap();
    writeln!(
        h,
        "#define MYNEWT_VAL_BLE_TRANSPORT_ACL_SIZE ({})",
        config.transport.acl_size
    )
    .unwrap();
    writeln!(
        h,
        "#define MYNEWT_VAL_BLE_TRANSPORT_EVT_COUNT ({})",
        config.transport.evt_count
    )
    .unwrap();
    writeln!(
        h,
        "#define MYNEWT_VAL_BLE_TRANSPORT_EVT_DISCARDABLE_COUNT ({})",
        config.transport.evt_discardable_count
    )
    .unwrap();
    writeln!(
        h,
        "#define MYNEWT_VAL_BLE_TRANSPORT_EVT_SIZE ({})",
        config.transport.evt_size
    )
    .unwrap();
    writeln!(h).unwrap();

    // Msys
    writeln!(h, "/* System mbuf pool */").unwrap();
    writeln!(
        h,
        "#define MYNEWT_VAL_MSYS_1_BLOCK_COUNT ({})",
        config.msys.block_count
    )
    .unwrap();
    writeln!(
        h,
        "#define MYNEWT_VAL_MSYS_1_BLOCK_SIZE ({})",
        config.msys.block_size
    )
    .unwrap();
    writeln!(h).unwrap();

    // GATT
    writeln!(h, "/* ATT / GATT */").unwrap();
    writeln!(
        h,
        "#define MYNEWT_VAL_BLE_ATT_PREFERRED_MTU ({})",
        config.gatt.preferred_mtu
    )
    .unwrap();
    writeln!(
        h,
        "#define MYNEWT_VAL_BLE_GATT_MAX_PROCS ({})",
        config.gatt.max_procs
    )
    .unwrap();
    writeln!(
        h,
        "#define MYNEWT_VAL_BLE_ATT_SVR_MAX_PREP_ENTRIES ({})",
        config.gatt.max_prep_entries
    )
    .unwrap();
    writeln!(
        h,
        "#define MYNEWT_VAL_BLE_GATT_RESUME_RATE ({})",
        config.gatt.resume_rate_ms
    )
    .unwrap();
    writeln!(h).unwrap();

    // L2CAP
    writeln!(h, "/* L2CAP */").unwrap();
    if config.l2cap.max_channels > 0 {
        writeln!(
            h,
            "#define MYNEWT_VAL_BLE_L2CAP_MAX_CHANS ({})",
            config.l2cap.max_channels
        )
        .unwrap();
    } else {
        writeln!(
            h,
            "#define MYNEWT_VAL_BLE_L2CAP_MAX_CHANS (3*MYNEWT_VAL_BLE_MAX_CONNECTIONS)"
        )
        .unwrap();
    }
    writeln!(
        h,
        "#define MYNEWT_VAL_BLE_L2CAP_SIG_MAX_PROCS ({})",
        config.l2cap.max_sig_procs
    )
    .unwrap();
    writeln!(h).unwrap();

    // Storage
    writeln!(h, "/* Bonding / Storage */").unwrap();
    writeln!(
        h,
        "#define MYNEWT_VAL_BLE_STORE_MAX_BONDS ({})",
        config.storage.max_bonds
    )
    .unwrap();
    writeln!(
        h,
        "#define MYNEWT_VAL_BLE_STORE_MAX_CCCDS ({})",
        config.storage.max_cccds
    )
    .unwrap();
    writeln!(h).unwrap();

    // Security Manager
    writeln!(h, "/* Security Manager */").unwrap();
    writeln!(
        h,
        "#define MYNEWT_VAL_BLE_SM_LEGACY ({})",
        config.security.legacy as u8
    )
    .unwrap();
    writeln!(
        h,
        "#define MYNEWT_VAL_BLE_SM_SC ({})",
        config.security.sc as u8
    )
    .unwrap();
    writeln!(
        h,
        "#define MYNEWT_VAL_BLE_SM_MITM ({})",
        config.security.mitm as u8
    )
    .unwrap();
    writeln!(
        h,
        "#define MYNEWT_VAL_BLE_SM_BONDING ({})",
        config.security.bonding as u8
    )
    .unwrap();
    writeln!(
        h,
        "#define MYNEWT_VAL_BLE_SM_MAX_PROCS ({})",
        config.security.max_procs
    )
    .unwrap();
    writeln!(h, "#define MYNEWT_VAL_BLE_SM_OUR_KEY_DIST (0)").unwrap();
    writeln!(h, "#define MYNEWT_VAL_BLE_SM_THEIR_KEY_DIST (0)").unwrap();
    writeln!(h).unwrap();

    writeln!(h, "#endif /* NIMBLE_CONFIG_OVERRIDE_H */").unwrap();

    let override_path = out_dir.join("nimble_config_override.h");
    fs::write(&override_path, h).expect("Failed to write nimble_config_override.h");
    override_path
}

fn main() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap());

    // Load nimble-config.toml and generate C override header.
    let config = load_config();
    let override_header = generate_syscfg_override(&config, &out_dir);

    // Download NimBLE source (cached in OUT_DIR between incremental builds).
    let nimble_dir = ensure_nimble_source(&out_dir);

    // Patch os_memblock_get to zero returned blocks (see doc comment for rationale).
    patch_os_mempool_zero_on_alloc(&nimble_dir);

    // Patch ble_hs.c to free Event structs before returning blocks to pool.
    patch_ble_hs_event_deinit_before_pool_put(&nimble_dir);

    // Local stub/override headers shipped with this crate.
    let stubs_dir = manifest_dir.join("nimble");

    // Rerun if local stubs change.
    println!("cargo:rerun-if-changed={}", stubs_dir.display());
    println!("cargo:rerun-if-changed=build.rs");

    // ----- Include paths -----
    // Stubs come FIRST so they shadow system headers (stdint.h, string.h, …).
    let mut include_dirs = vec![
        stubs_dir.clone(),
        nimble_dir.join("nimble/include"),
        nimble_dir.join("nimble/host/include"),
        nimble_dir.join("porting/nimble/include"),
        nimble_dir.join("nimble/transport/include"),
    ];

    if config.security.legacy || config.security.sc {
        include_dirs.push(nimble_dir.join("ext/tinycrypt/include"));
    }

    let exclude_headers = ["porting/nimble/include/syscfg/syscfg.h"];

    // ----- Bindgen -----
    let headers = include_dirs
        .iter()
        .flat_map(|dir| WalkDir::new(dir).into_iter())
        .filter_map(Result::ok)
        .filter(|e| e.path().extension().and_then(|s| s.to_str()) == Some("h"))
        .map(|e| e.path().display().to_string())
        .filter(|header| !exclude_headers.iter().any(|excl| header.contains(excl)));

    let bindings = bindgen::Builder::default()
        .headers(headers)
        .clang_args(
            include_dirs
                .iter()
                .map(|dir| format!("-I{}", dir.display())),
        )
        // Include the config override header before everything else so our
        // MYNEWT_VAL_* defines take precedence over syscfg.h defaults.
        .clang_arg(format!("-include{}", override_header.display()))
        .clang_arg("-DBLE_NPL_LOG_MODULE=BLE_HS_LOG")
        // Target bare-metal RISC-V so clang does not search for host system headers.
        .clang_arg("--target=riscv32-unknown-none-elf")
        .clang_arg("-ffreestanding")
        .clang_arg("-nostdlibinc")
        .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()))
        .use_core()
        .derive_debug(true)
        .ctypes_prefix("core::ffi")
        .merge_extern_blocks(true)
        .generate()
        .expect("Unable to generate NimBLE bindings");

    bindings
        .write_to_file(out_dir.join("nimble_host_bindings.rs"))
        .expect("Could not write NimBLE bindings");

    // ----- Compile Apache NimBLE host as a static library -----
    let mut cc_build = cc::Build::new();

    // Collect all .c files from nimble/host/src/
    let host_sources = WalkDir::new(nimble_dir.join("nimble/host/src"))
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.path().extension().and_then(|s| s.to_str()) == Some("c"))
        .map(|e| e.path().to_path_buf());

    cc_build
        .files(host_sources)
        // Porting essentials
        .file(nimble_dir.join("porting/nimble/src/endian.c"))
        .file(nimble_dir.join("porting/nimble/src/os_mbuf.c"))
        .file(nimble_dir.join("porting/nimble/src/os_mempool.c"))
        .file(nimble_dir.join("porting/nimble/src/os_msys.c"))
        .file(nimble_dir.join("porting/nimble/src/mem.c"))
        .file(nimble_dir.join("porting/nimble/src/nimble_port.c"))
        .file(nimble_dir.join("nimble/transport/src/transport.c"))
        .includes(&include_dirs);

    if config.security.legacy || config.security.sc {
        cc_build
            .file(nimble_dir.join("ext/tinycrypt/src/aes_encrypt.c"))
            .file(nimble_dir.join("ext/tinycrypt/src/utils.c"));
    }

    // Cross-compile for bare-metal RISC-V
    cc_build
        .compiler("clang")
        // Include config overrides before all NimBLE source files.
        .flag(format!("-include{}", override_header.display()))
        .flag("--target=riscv32-unknown-none-elf")
        .flag("-march=rv32imac")
        .flag("-mabi=ilp32")
        .flag("-ffreestanding")
        .flag("-fno-builtin")
        .flag("-fdata-sections")
        .flag("-ffunction-sections")
        .flag("-Os")
        .flag("-g0")
        .flag("-w") // Suppress all C warnings from third-party NimBLE code
        .compile("apache_nimble_host");

    println!("cargo:rustc-link-lib=static=apache_nimble_host");
    println!("cargo:rustc-link-search=native={}", out_dir.display());
}
