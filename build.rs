fn main() {
    slint_build::compile("ui/app.slint").unwrap();
    // Explorer / taskbar icon + version info for rmpyou.exe
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        winresource::WindowsResource::new().set_icon("ui/app.ico").compile().unwrap();
    }
}
