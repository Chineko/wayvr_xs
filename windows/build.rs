fn main() {
    assert_eq!(
        std::env::var("CARGO_CFG_TARGET_OS").unwrap(),
        "windows",
        "Build this application on Windows"
    );
    assert_eq!(
        std::env::var("CARGO_CFG_TARGET_ARCH").unwrap(),
        "x86_64",
        "SteamVR requires the x64 build"
    );
    cc::Build::new()
        .cpp(true)
        .file("native/steamvr.cpp")
        .include("vendor/openvr")
        .flag_if_supported("/std:c++17")
        .compile("xs_steamvr");
    println!("cargo:rerun-if-changed=native/steamvr.cpp");
    println!("cargo:rerun-if-changed=vendor/openvr/openvr.h");
}
