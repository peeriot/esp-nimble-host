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

fn main() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap());

    // Download NimBLE source (cached in OUT_DIR between incremental builds).
    let nimble_dir = ensure_nimble_source(&out_dir);

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
