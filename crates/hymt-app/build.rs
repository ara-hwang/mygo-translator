//! Embeds the Windows resources (app icon and file metadata) into the `hymt` binary.
//!
//! GPUI's Windows platform loads the module's icon resource with ID 1 for the window class
//! (`LoadImageW(..., PCWSTR(1), IMAGE_ICON, ...)` in `gpui-pre-windows`), so the icon file
//! must be the first icon in the resource file metadata. See `assets/icon-512.png` and
//! `scripts/make-icons.py` for how the `.ico` is produced.

fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut resources = winresource::WindowsResource::new();
        resources
            .set_icon("assets/hymt.ico")
            .set("ProductName", "HyMT")
            .set("FileDescription", "HyMT — 오프라인 번역");
        resources
            .compile()
            .expect("failed to compile Windows resources");
    }
}
