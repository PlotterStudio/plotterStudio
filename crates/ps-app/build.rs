//! Embeds the application icon and version information in the Windows executable, so Explorer, the
//! taskbar and shortcuts show Plotter Studio's icon instead of a generic one.

fn main() {
    #[cfg(windows)]
    embed_windows_resources();
}

#[cfg(windows)]
fn embed_windows_resources() {
    let mut resources = winresource::WindowsResource::new();
    resources.set_icon("icons/icon.ico");
    resources.set("ProductName", "Plotter Studio");
    resources.set("FileDescription", "Plotter Studio");
    resources
        .compile()
        .expect("embedding the icon in the Windows executable");
}
