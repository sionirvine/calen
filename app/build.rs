fn main() {
    // Embed the app icon + version info into the Windows executable.
    #[cfg(windows)]
    {
        let icon = std::path::Path::new("../assets/calen.ico");
        println!("cargo:rerun-if-changed=../assets/calen.ico");
        if icon.exists() {
            let mut res = winresource::WindowsResource::new();
            res.set_icon(icon.to_str().unwrap());
            res.set("ProductName", "Calen");
            res.set("FileDescription", "Calen calendar");
            if let Err(e) = res.compile() {
                println!("cargo:warning=couldn't embed icon: {e}");
            }
        }
    }
}
