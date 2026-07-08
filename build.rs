//! Compiles the Objective-C helpers in `src/native/` into the cargo build and
//! links the macOS frameworks they use. All OS interaction lives in those `.m`
//! files; Rust calls them through plain C functions (see rust-objc.md).

fn main() {
    println!("cargo:rerun-if-changed=src/native");
    cc::Build::new()
        .file("src/native/notifications.m")
        .file("src/native/camera.m")
        .file("src/native/tray.m")
        .flag("-fobjc-arc")
        .compile("native");

    // rustc drives the final link, so it won't add these automatically the way
    // clang does when it links .m files itself.
    println!("cargo:rustc-link-lib=objc");
    for framework in ["Foundation", "AppKit", "UserNotifications", "CoreMediaIO"] {
        println!("cargo:rustc-link-lib=framework={framework}");
    }
}
