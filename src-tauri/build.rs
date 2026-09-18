fn main() {
    // Windows: a `cargo test` binary is linked without an application
    // manifest, so the Common-Controls v6 import the file dialog pulls in
    // (`TaskDialogIndirect`) has nothing to resolve against and the executable
    // fails to start before a single test runs — exit code 0xC0000139.
    //
    // The app binary never had this problem: `tauri_build::build()` below
    // embeds its manifest. This declares the same dependency everywhere else,
    // so what the tests link is what the app links.
    //
    // `rustc-link-arg-tests` would be the narrower flag, but Cargo does not
    // apply it to a library's own unit tests — which is the binary that fails.
    #[cfg(windows)]
    {
        println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
        println!(
            "cargo:rustc-link-arg=/MANIFESTDEPENDENCY:type='win32' \
             name='Microsoft.Windows.Common-Controls' version='6.0.0.0' \
             processorArchitecture='*' publicKeyToken='6595b64144ccf1df' language='*'"
        );
        // The app binary already carries a manifest, embedded as a resource by
        // `tauri_build` below. Letting the linker embed a second one there is a
        // hard error (CVT1100), so the app opts back out — this last `/MANIFEST`
        // wins for that target and leaves Tauri's own manifest alone.
        println!("cargo:rustc-link-arg-bins=/MANIFEST:NO");
    }
    tauri_build::build()
}
