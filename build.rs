use std::path::Path;
use walkdir::WalkDir;

fn main() {
    // Build paths
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut repo_root = manifest_dir.to_path_buf();
    repo_root.pop();
    let nimble_dir = repo_root.join("vendor/apache-nimble");
    if !nimble_dir.exists() {
        panic!("Couldn't find NimBLE in 'vendor/apache-nimble'. Did you update the git submodule?");
    }

    // Rerun if local impl changed
    println!(
        "cargo:rerun-if-changed={}",
        manifest_dir.join("nimble").display()
    );
    // Rerun if submodule changed
    println!("cargo:rerun-if-changed={}", nimble_dir.display());

    // Gather includes for bindgen
    let include_dirs = vec![
        manifest_dir.join("nimble"),
        nimble_dir.join("nimble/include"),
        nimble_dir.join("nimble/host/include"),
        nimble_dir.join("porting/nimble/include"),
        nimble_dir.join("nimble/transport/include"),
        repo_root.join("wamr-sys/platform/embassy"),
    ];
    let exclude_headers = vec!["porting/nimble/include/syscfg/syscfg.h"];
    let headers = include_dirs
        .iter()
        .map(|dir| WalkDir::new(dir).into_iter())
        .flatten()
        .filter_map(Result::ok)
        .filter(|e| e.path().extension().and_then(|s| s.to_str()) == Some("h"))
        .map(|e| e.path().display().to_string())
        .filter(|header| {
            for exclusion in exclude_headers.iter() {
                if header.contains(exclusion) {
                    return false;
                }
            }

            true
        });

    // use bindgen to generate bindings from the header
    let bindings = bindgen::Builder::default()
        .headers(headers)
        .clang_args(
            include_dirs
                .iter()
                .map(|dir| format!("-I{}", dir.display())),
        )
        .clang_arg("-DBLE_NPL_LOG_MODULE=BLE_HS_LOG")
        .parse_callbacks(Box::new(bindgen::CargoCallbacks::new())) // this adds rerun hooks for cargo
        .use_core() // to make bindgen not use std types
        .derive_debug(true)
        .ctypes_prefix("core::ffi") // to make bindgen use core::ffi::c_types instead of std::os::raw::c_types - if we are fancy, we could give it a module we implemented here
        .merge_extern_blocks(true)
        .generate()
        .expect("unable to generate bindings");

    // write the bindings to the output directory (this is where we put generated files when working with cargo)
    let out_path = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap());
    bindings
        .write_to_file(out_path.join("nimble_host_bindings.rs"))
        .expect("could not write nimble bindings");

    // Build the Apache-NimBLE Host as a static library
    let mut cc_build = cc::Build::new();

    // Add files, includes and defines
    let source_dirs = vec![nimble_dir.join("nimble/host/src")];
    let sources = source_dirs
        .iter()
        .map(|dir| WalkDir::new(dir).into_iter())
        .flatten()
        .filter_map(Result::ok)
        .filter(|e| e.path().extension().and_then(|s| s.to_str()) == Some("c"))
        .map(|e| e.path().display().to_string());

    cc_build
        .files(sources)
        // Porting essentials
        .file(nimble_dir.join("porting/nimble/src/endian.c"))
        .file(nimble_dir.join("porting/nimble/src/os_mbuf.c"))
        .file(nimble_dir.join("porting/nimble/src/os_mempool.c"))
        .file(nimble_dir.join("porting/nimble/src/os_msys.c"))
        .file(nimble_dir.join("porting/nimble/src/mem.c"))
        .file(nimble_dir.join("porting/nimble/src/nimble_port.c"))
        .file(nimble_dir.join("nimble/transport/src/transport.c"))
        // .flag("-H") // This outputs all the headers. TODO Double check that we are only using the minimum necessary
        .include(repo_root.join("wamr-sys/platform/embassy"))
        .includes(include_dirs);

    // // Compile with appropriate options
    // cc_build
    //     .compiler("clang")
    //     .flag("--target=riscv32-unknown-none-elf")
    //     .flag("-march=rv32imac")
    //     .flag("-mabi=ilp32")
    //     .flag("-ffreestanding")
    //     .flag("-fno-builtin")
    //     .flag("-fdata-sections")
    //     .flag("-ffunction-sections")
    //     .flag("-Os")
    //     .flag("-g0")
    //     .flag("-Wno-unused-parameter")
    //     .flag("-Wno-unused-variable")
    //     .compile("apache_nimble_host");

    // Debug-friendly C build options (ESP32-C6 / riscv32-unknown-none-elf)
    cc_build
        .compiler("clang")
        .flag("--target=riscv32-unknown-none-elf")
        .flag("-march=rv32imac")
        .flag("-mabi=ilp32")
        .flag("-ffreestanding")
        .flag("-fno-builtin")
        // Debugging
        .flag("-O0") // easier stepping, no surprises
        .flag("-g3") // maximum debug info
        .flag("-fno-omit-frame-pointer") // better backtraces
        .flag("-fno-optimize-sibling-calls")
        // Make debug builds less "clever"
        .flag("-fno-strict-aliasing")
        // Optional, but often helpful
        .flag("-Wall")
        .flag("-Wextra")
        .flag("-Wundef")
        // Keep or drop these depending on your link setup:
        // In debug, dropping them avoids "why did my function disappear?"
        // .flag("-fdata-sections")
        // .flag("-ffunction-sections")
        // You can keep your noise suppression if you want
        .flag("-Wno-unused-parameter")
        .flag("-Wno-unused-variable")
        .compile("apache_nimble_host");

    println!("cargo:rustc-link-lib=static=apache_nimble_host"); // link the static library to the final binary
    println!("cargo:rustc-link-search=native={}", out_path.display()); // search for the static library in the output directory (so that we know where the file is we just mentioned)
}
