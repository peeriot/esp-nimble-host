use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};

use flate2::read::GzDecoder;
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

fn main() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap());

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
    let include_dirs = vec![
        stubs_dir.clone(),
        nimble_dir.join("nimble/include"),
        nimble_dir.join("nimble/host/include"),
        nimble_dir.join("porting/nimble/include"),
        nimble_dir.join("nimble/transport/include"),
    ];

    let exclude_headers = vec!["porting/nimble/include/syscfg/syscfg.h"];

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

    // Cross-compile for bare-metal RISC-V
    cc_build
        .compiler("clang")
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
