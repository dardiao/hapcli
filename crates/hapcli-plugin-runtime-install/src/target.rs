// Copyright (C) 2026 AnalyseDeCircuit

//! Host target selection for sidecar release assets.

pub(super) fn wasm_runtime_target_triple() -> Result<&'static str, String> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => Ok("aarch64-apple-darwin"),
        ("macos", "x86_64") => Ok("x86_64-apple-darwin"),
        ("linux", "x86_64") => Ok("x86_64-unknown-linux-gnu"),
        ("linux", "aarch64") => Ok("aarch64-unknown-linux-gnu"),
        ("windows", "x86_64") => Ok("x86_64-pc-windows-msvc"),
        ("windows", "aarch64") => Ok("aarch64-pc-windows-msvc"),
        (os, arch) => Err(format!("No Wasm runtime sidecar target for {os}/{arch}")),
    }
}

pub(super) fn wasm_runtime_binary_name() -> &'static str {
    if cfg!(target_os = "windows") {
        "hapcli-wasm-runtime.exe"
    } else {
        "hapcli-wasm-runtime"
    }
}
