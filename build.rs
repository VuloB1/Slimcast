// Embeds the app icon as resource id 1, which gpui loads for the window and Windows shows for the .exe.
fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") { return }
    let mut res = winresource::WindowsResource::new();
    res.set_icon("assets/slimcast.ico");
    res.compile().expect("embed icon");
}
