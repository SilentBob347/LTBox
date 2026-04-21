fn main() {
    // Lucide icon subset codegen. Reads `fonts/lucide.toml`, subsets
    // the bundled `lucide.ttf` to just the declared glyphs, and
    // writes `src/icon.rs` with one `Text`-returning function per
    // entry. Rerun only when the TOML changes.
    println!("cargo:rerun-if-changed=fonts/lucide.toml");
    iced_lucide::build("fonts/lucide.toml").expect("Failed to generate Lucide icon module");
    // iced_lucide is written against iced's git HEAD where
    // `Font::new(&'static str)` exists. iced 0.14 on crates.io
    // renamed that ctor to `Font::with_name`, so patch the generated
    // module to match before rustc consumes it.
    {
        let path = std::path::Path::new("src/icon.rs");
        if let Ok(src) = std::fs::read_to_string(path) {
            let patched = src.replace("Font::new(", "Font::with_name(");
            if patched != src {
                std::fs::write(path, patched).expect("Failed to patch icon.rs for iced 0.14");
            }
        }
    }

    #[cfg(target_os = "windows")]
    {
        // Override the defaults that `winresource` derives from the Cargo
        // package name (`ltbox-gui`) so Explorer / Task Manager / the
        // taskbar show "LTBox" instead of the crate name.
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/icon.ico");
        res.set("ProductName", "LTBox");
        res.set("FileDescription", "LTBox");
        res.set("InternalName", "LTBox");
        res.set("OriginalFilename", "ltbox.exe");
        res.compile().expect("Failed to compile Windows resources");

        // Bump the main-thread stack reserve to 8 MB on Windows. The
        // default 1 MB is too tight for debug builds: iced + cosmic-text
        // shaping with the Noto Sans CJK bundle and the deeply nested
        // widget trees in the wizard exec screens blow past 1 MB and
        // trip `STATUS_STACK_OVERFLOW` (0xc00000fd) when a wizard's
        // confirm step pushes the app into the exec view.
        println!("cargo:rustc-link-arg=/STACK:8388608");
    }
}
