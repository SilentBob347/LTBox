fn main() {
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
