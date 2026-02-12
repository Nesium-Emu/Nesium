fn main() {
    // Only embed Windows icon when building for Windows target on a Windows host
    #[cfg(target_os = "windows")]
    {
        // Check the actual target OS (not host OS) to avoid embedding
        // Windows resources when cross-compiling for Android
        let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
        if target_os == "windows" {
            let mut res = winres::WindowsResource::new();
            res.set_icon("resources/NESIUM.ico");
            if let Err(e) = res.compile() {
                eprintln!("Warning: Failed to embed icon in executable: {}", e);
                eprintln!(
                    "The application will still work, but the .exe may not have the custom icon."
                );
            } else {
                println!("cargo:warning=Successfully embedded NESIUM.ico in Windows executable");
            }
        }
    }
}
