use std::fs;

fn main() {
    // Extract wasmtime version from Cargo.lock for use in version.rs
    let lock_content = fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().join("Cargo.lock"),
    )
    .unwrap_or_default();

    let wasmtime_version = extract_wasmtime_version(&lock_content).unwrap_or("unknown");
    println!("cargo:rustc-env=WASMTIME_VERSION={wasmtime_version}");
    println!("cargo:rerun-if-changed=../Cargo.lock");
}

fn extract_wasmtime_version(lock: &str) -> Option<&str> {
    let mut lines = lock.lines();
    while let Some(line) = lines.next() {
        if line.trim() == r#"name = "wasmtime""# {
            if let Some(ver_line) = lines.next() {
                let ver_line = ver_line.trim();
                if let Some(ver) = ver_line.strip_prefix("version = \"") {
                    return ver.strip_suffix('"');
                }
            }
        }
    }
    None
}
