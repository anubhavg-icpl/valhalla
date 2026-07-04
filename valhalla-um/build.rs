// Embed the Valhalla icon into the client executable so it appears in the
// taskbar, title bar, and file explorer.
fn main() {
    if std::path::Path::new("installer/assets/logo.ico").exists() {
        let mut res = winres::WindowsResource::new();
        res.set_icon("installer/assets/logo.ico");
        if let Err(e) = res.compile() {
            println!("cargo:warning=winres compile failed: {e}");
        }
    }
}
