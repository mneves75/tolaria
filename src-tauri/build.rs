fn main() {
    ensure_mcp_resource_dir();
    compile_apple_notes_proto();
    tauri_build::build()
}

// Ensure resource directory exists for the Tauri build.
// Gitignored and populated by bundle-mcp-server.mjs.
// Without a placeholder, `tauri build` / `cargo test` fails if the script hasn't run.
fn ensure_mcp_resource_dir() {
    let path = std::path::Path::new("resources/mcp-server");
    if !path.exists() {
        std::fs::create_dir_all(path).ok();
        std::fs::write(path.join(".placeholder"), "").ok();
    }
}

// Generate Rust types for the Apple Notes note-body protobuf. Uses protox
// (pure Rust) so the build needs no system `protoc`.
fn compile_apple_notes_proto() {
    println!("cargo:rerun-if-changed=proto/notestore.proto");
    let descriptors = protox::compile(["proto/notestore.proto"], ["proto"])
        .expect("compile proto/notestore.proto");
    prost_build::Config::new()
        .compile_fds(descriptors)
        .expect("generate Apple Notes protobuf types");
}
