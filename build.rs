use std::fs;

/// The resource script and manifest cannot read `Cargo.toml`, so their version
/// numbers are checked here. A forgotten edit fails the build rather than
/// shipping an exe that reports the wrong version.
fn check_versions() {
    let version = env!("CARGO_PKG_VERSION");
    let numbers = version.replace('.', ",");
    let script = fs::read_to_string("assets/app.rc").expect("assets/app.rc is missing");
    let manifest =
        fs::read_to_string("assets/app.manifest").expect("assets/app.manifest is missing");

    let in_step = script.contains(&format!("FILEVERSION {numbers},0"))
        && script.contains(&format!("PRODUCTVERSION {numbers},0"))
        && script.contains(&format!("\"FileVersion\", \"{version}\""))
        && script.contains(&format!("\"ProductVersion\", \"{version}\""))
        && manifest.contains(&format!("version=\"{version}.0\""));
    assert!(
        in_step,
        "update the version in assets/app.rc and assets/app.manifest to {version}"
    );
}

fn main() {
    println!("cargo:rerun-if-changed=assets/app.rc");
    println!("cargo:rerun-if-changed=assets/app.manifest");
    println!("cargo:rerun-if-changed=assets/vitals.ico");
    check_versions();
    embed_resource::compile("assets/app.rc", embed_resource::NONE)
        .manifest_required()
        .expect("failed to compile Windows resources");

    // Resolve this exe's imports from System32 only (LOAD_LIBRARY_SEARCH_SYSTEM32).
    // By default Windows looks in the exe's own folder first for any DLL outside
    // its KnownDLLs list, which includes d2d1, dwrite, dwmapi, dxgi and pdh. A
    // malicious copy of one sitting in Downloads would otherwise run inside this
    // process, and with admin rights in the elevated helper.
    println!("cargo:rustc-link-arg-bins=/DEPENDENTLOADFLAG:0x800");
}
