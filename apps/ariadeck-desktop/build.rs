//! Build script for ariadeck-desktop.
//!
//! Responsibilities:
//!   1. Render `assets/icon.svg` to PNG at each standard ICO size (16–256).
//!   2. Pack those PNGs into a multi-resolution `icon.ico` in `OUT_DIR`.
//!   3. Render the SVG at 32×32 and write `tray_icon.rgba` to `OUT_DIR`
//!      (consumed by `platform.rs` via `include_bytes!(concat!(env!("OUT_DIR"), …))`).
//!   4. On Windows, embed the ICO in the binary via a resource script.

use std::{
    fs::{self, File},
    io::BufWriter,
    path::PathBuf,
};

fn main() {
    let manifest_dir = PathBuf::from(
        std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR set by cargo"),
    );
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR set by cargo"));
    let svg_path = manifest_dir.join("assets").join("icon.svg");

    println!("cargo:rerun-if-changed=assets/icon.svg");

    let svg_data = fs::read(&svg_path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", svg_path.display()));

    // ── Tray icon: 32×32 raw RGBA ─────────────────────────────────────────────
    let tray_rgba = render_rgba(&svg_data, 32, 32);
    let tray_path = out_dir.join("tray_icon.rgba");
    fs::write(&tray_path, &tray_rgba)
        .unwrap_or_else(|e| panic!("failed to write tray_icon.rgba: {e}"));

    // ── Multi-resolution ICO ──────────────────────────────────────────────────
    let ico_path = out_dir.join("icon.ico");
    build_ico(&svg_data, &ico_path);

    // ── Linux/X11: 128×128 RGBA for WindowOptions::icon ─────────────────────
    let window_icon_rgba = render_rgba(&svg_data, 128, 128);
    fs::write(out_dir.join("window_icon.rgba"), &window_icon_rgba)
        .unwrap_or_else(|e| panic!("failed to write window_icon.rgba: {e}"));

    // ── Windows: embed ICO + version/product metadata (RELEASE-001) ───────────
    #[cfg(target_os = "windows")]
    {
        let mut res = winres::WindowsResource::new();
        res.set_icon(ico_path.to_str().expect("OUT_DIR path must be valid UTF-8"));
        let version = std::env::var("CARGO_PKG_VERSION").unwrap_or_else(|_| "0.0.0".into());
        // winres expects dotted numeric FileVersion; append .0 when needed.
        let file_version = if version.split('.').count() >= 4 {
            version.clone()
        } else {
            format!("{version}.0")
        };
        res.set("ProductName", "AriaDeck");
        res.set("FileDescription", "AriaDeck — native aria2 desktop client");
        res.set("CompanyName", "AriaDeck contributors");
        res.set("LegalCopyright", "Copyright (c) AriaDeck contributors");
        res.set("OriginalFilename", "ariadeck-desktop.exe");
        res.set("ProductVersion", &version);
        res.set("FileVersion", &file_version);
        res.compile().expect("winres: failed to compile resources");
    }
}

/// Render `svg_data` into a `width × height` RGBA byte buffer using resvg.
fn render_rgba(svg_data: &[u8], width: u32, height: u32) -> Vec<u8> {
    use resvg::{tiny_skia, usvg};

    let opt = usvg::Options::default();
    let tree = usvg::Tree::from_data(svg_data, &opt).expect("failed to parse icon.svg");

    let svg_size = tree.size();
    let scale_x = width as f32 / svg_size.width();
    let scale_y = height as f32 / svg_size.height();
    let transform = tiny_skia::Transform::from_scale(scale_x, scale_y);

    let mut pixmap = tiny_skia::Pixmap::new(width, height).expect("failed to create pixmap");
    resvg::render(&tree, transform, &mut pixmap.as_mut());

    pixmap.data().to_vec()
}

/// Build a multi-resolution `.ico` from the SVG and write it to `out_path`.
fn build_ico(svg_data: &[u8], out_path: &std::path::Path) {
    let mut icon_dir = ico::IconDir::new(ico::ResourceType::Icon);

    // Standard ICO sizes expected by Windows and cross-platform tooling.
    for &size in &[16u32, 24, 32, 48, 64, 128, 256] {
        let rgba = render_rgba(svg_data, size, size);
        let image = ico::IconImage::from_rgba_data(size, size, rgba);
        icon_dir.add_entry(
            ico::IconDirEntry::encode(&image)
                .unwrap_or_else(|e| panic!("failed to encode {size}×{size} ICO entry: {e}")),
        );
    }

    let file = BufWriter::new(
        File::create(out_path)
            .unwrap_or_else(|e| panic!("failed to create {}: {e}", out_path.display())),
    );
    icon_dir
        .write(file)
        .unwrap_or_else(|e| panic!("failed to write ICO: {e}"));
}
