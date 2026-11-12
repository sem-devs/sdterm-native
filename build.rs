//! Windows only: puts the icon and the version information into the .exe, so Explorer, the taskbar
//! and "Apps and features" show them. Nothing happens on other systems.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=resources/icon.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        windows_resources();
    }
}

#[cfg(windows)]
fn windows_resources() {
    let mut res = winresource::WindowsResource::new();
    res.set_icon("resources/icon.ico");
    res.set("ProductName", "sdterm native");
    res.set("FileDescription", "sdterm native");
    res.set("CompanyName", "SEM Devs");
    res.set("LegalCopyright", "SEM Devs, MIT license");
    res.set("OriginalFilename", "sdterm-native.exe");
    if let Err(err) = res.compile() {
        // The program works without it, so a missing resource compiler is only a warning.
        println!("cargo:warning=could not embed the Windows resources: {err}");
    }
}

#[cfg(not(windows))]
fn windows_resources() {}
