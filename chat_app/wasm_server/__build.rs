fn main() {
    // use std::fs;
    use std::process::Command;

    let proj_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();

    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=../wasm_client");

    println!("STARTING BUILD SCRIPT");

    // Build the wasm application
    let wasm_client_dir = std::path::Path::new(&proj_dir)
        .parent()
        .unwrap()
        .join("wasm_client");

    //println!("Building wasm client in: {}", wasm_client_dir.display());

    // note: this DOESN'T WORK, because it tries to lock the /target folder (which is already locked by this build script).
    let output = Command::new("trunk")
        .args(&["build", "--release"])
        .current_dir(wasm_client_dir)
        .output()
        .expect("Failed to build wasm application");

    if !output.status.success() {
        panic!(
            "Wasm build failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
