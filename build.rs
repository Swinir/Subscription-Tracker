//! Puts the app's icon (and its name) in the Windows program, so Explorer and the taskbar show them.

fn main() {
    println!("cargo:rerun-if-changed=assets/icon.ico");
    println!("cargo:rerun-if-changed=build.rs");
    #[cfg(windows)]
    {
        let mut resource = winresource::WindowsResource::new();
        resource.set_icon("assets/icon.ico");
        resource.set("ProductName", "Subscription Tracker");
        resource.set("FileDescription", "Subscription Tracker");
        if let Err(e) = resource.compile() {
            // Without the Windows SDK's resource compiler the program still works, only without the icon in the file.
            println!("cargo:warning=couldn't add the icon to the program: {e}");
        }
    }
}
