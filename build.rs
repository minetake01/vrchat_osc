fn main() {
    println!("cargo:rustc-check-cfg=cfg(apple_or_bsd_or_illumos)");

    let target_os = std::env::var("CARGO_CFG_TARGET_OS").map(|s| s.to_lowercase()).unwrap_or_default();
    let target_vendor = std::env::var("CARGO_CFG_TARGET_VENDOR").map(|s| s.to_lowercase()).unwrap_or_default();

    let is_apple = target_vendor == "apple"
        && matches!(
            target_os.as_str(),
            "macos" | "ios" | "tvos" | "watchos" | "visionos"
        );
    let is_unsupported_bsd_or_illumos =
        matches!(target_os.as_str(), "freebsd" | "netbsd" | "openbsd" | "illumos");

    if is_apple || is_unsupported_bsd_or_illumos {
        println!("cargo:rustc-cfg=apple_or_bsd_or_illumos");
    }
}
