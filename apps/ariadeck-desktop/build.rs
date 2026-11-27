//! Build script for ariadeck-desktop.
//!
//! Responsibilities:
//!   1. Parse `assets/icon.svg` once into a `usvg::Tree`.
//!   2. Render the 32x32 RGBA tray icon (`tray_icon.rgba`) and 128x128 window
//!      icon (`window_icon.rgba`) into `OUT_DIR`.
//!   3. On Windows targets (`CARGO_CFG_TARGET_OS == "windows"`), pack a
//!      multi-resolution `icon.ico` and embed it with sanitized numeric version
//!      metadata via `winres`.
//!   4. On macOS targets (`CARGO_CFG_TARGET_OS == "macos"`), emit a
//!      multi-resolution PNG-backed `icon.icns` into `OUT_DIR` for app bundle
//!      packaging (`scripts/package-macos.sh`).

use std::{
    fs::{self, File},
    io::{BufWriter, Write as _},
    path::{Path, PathBuf},
};

use resvg::{tiny_skia, usvg};

fn main() {
    println!("cargo:rerun-if-env-changed=ARIADECK_BUILD_CHANNEL");
    let build_channel = std::env::var("ARIADECK_BUILD_CHANNEL").unwrap_or_else(|_| "dev".into());
    println!("cargo:rustc-env=ARIADECK_BUILD_CHANNEL={build_channel}");

    let manifest_dir = PathBuf::from(
        std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR set by cargo"),
    );
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR set by cargo"));
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let svg_path = manifest_dir.join("assets").join("icon.svg");

    println!("cargo:rerun-if-changed=assets/icon.svg");

    let svg_data = fs::read(&svg_path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", svg_path.display()));
    let opt = usvg::Options::default();
    let tree = usvg::Tree::from_data(&svg_data, &opt).expect("failed to parse icon.svg");

    // ── Tray monochrome template silhouettes (straight RGBA for OS tray APIs) ─
    for (filename, fill, bar_opacity) in [
        ("tray_template_dark.rgba", "#FFFFFF", "0.78"),
        ("tray_template_light.rgba", "#18181B", "0.78"),
    ] {
        let template_svg = format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 512 512">
  <g transform="translate(256 256) scale(1.32) translate(-256 -244)">
    <path fill-rule="evenodd" fill="{fill}" d="M 256 76 L 416 298 L 96 298 Z M 256 168 L 168 298 L 344 298 Z"/>
    <rect x="116" y="320" width="280" height="42" rx="21" fill="{fill}"/>
    <rect x="156" y="378" width="200" height="42" rx="21" fill="{fill}" opacity="{bar_opacity}"/>
  </g>
</svg>"#
        );
        let template_tree = usvg::Tree::from_data(template_svg.as_bytes(), &opt)
            .expect("failed to parse tray template svg");
        let rgba = render_pixmap(&template_tree, 32, 32).take_demultiplied();
        fs::write(out_dir.join(filename), &rgba)
            .unwrap_or_else(|e| panic!("failed to write {filename}: {e}"));
    }

    // ── Window icon: 128×128 raw RGBA (Linux/X11 WindowOptions::icon) ─────────
    let window_icon_rgba = render_pixmap(&tree, 128, 128).data().to_vec();
    fs::write(out_dir.join("window_icon.rgba"), &window_icon_rgba)
        .unwrap_or_else(|e| panic!("failed to write window_icon.rgba: {e}"));

    // ── Windows: multi-resolution ICO + version/product metadata ──────────────
    if target_os == "windows" {
        let ico_path = out_dir.join("icon.ico");
        build_ico(&tree, &ico_path);

        let mut res = winres::WindowsResource::new();
        res.set_icon(ico_path.to_str().expect("OUT_DIR path must be valid UTF-8"));
        let version = std::env::var("CARGO_PKG_VERSION").unwrap_or_else(|_| "0.0.0".into());
        let file_version = numeric_file_version(&version);
        res.set("ProductName", "AriaDeck");
        res.set("FileDescription", "AriaDeck — native aria2 desktop client");
        res.set("CompanyName", "AriaDeck contributors");
        res.set("LegalCopyright", "Copyright (c) AriaDeck contributors");
        res.set("OriginalFilename", "ariadeck-desktop.exe");
        res.set("ProductVersion", &version);
        res.set("FileVersion", &file_version);
        res.compile().expect("winres: failed to compile resources");
    }

    // ── macOS: multi-resolution PNG-backed ICNS for .app bundle ───────────────
    if target_os == "macos" {
        let icns_path = out_dir.join("icon.icns");
        build_icns(&tree, &icns_path);
    }
}

/// Strip `-prerelease` / `+build` suffixes and normalize to 4 dotted numeric
/// components required by Windows `VS_FIXEDFILEINFO` / `FileVersion`.
fn numeric_file_version(version: &str) -> String {
    let core = version.split(['-', '+']).next().unwrap_or("0.0.0");
    let mut parts: Vec<u16> = core
        .split('.')
        .take(4)
        .map(|part| part.parse::<u16>().unwrap_or(0))
        .collect();
    while parts.len() < 4 {
        parts.push(0);
    }
    format!("{}.{}.{}.{}", parts[0], parts[1], parts[2], parts[3])
}

/// Render a parsed SVG `tree` into a `width × height` `tiny_skia::Pixmap`.
fn render_pixmap(tree: &usvg::Tree, width: u32, height: u32) -> tiny_skia::Pixmap {
    let svg_size = tree.size();
    let scale_x = width as f32 / svg_size.width();
    let scale_y = height as f32 / svg_size.height();
    let transform = tiny_skia::Transform::from_scale(scale_x, scale_y);

    let mut pixmap = tiny_skia::Pixmap::new(width, height).expect("failed to create pixmap");
    resvg::render(tree, transform, &mut pixmap.as_mut());
    pixmap
}

/// Build a multi-resolution `.ico` from the parsed SVG tree and write it to `out_path`.
fn build_ico(tree: &usvg::Tree, out_path: &Path) {
    let mut icon_dir = ico::IconDir::new(ico::ResourceType::Icon);

    // Standard ICO sizes expected by Windows and cross-platform tooling.
    for &size in &[16u32, 24, 32, 48, 64, 128, 256] {
        let rgba = render_pixmap(tree, size, size).data().to_vec();
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

/// Build an Apple `.icns` container wrapping standard PNG-backed icon entries.
fn build_icns(tree: &usvg::Tree, out_path: &Path) {
    // Modern PNG-backed OSTypes supported natively on macOS 12+:
    // icp4 (16), icp5 (32), icp6 (64), ic07 (128), ic08 (256), ic09 (512),
    // ic10 (1024 / 512@2x), ic11 (32 / 16@2x), ic12 (64 / 32@2x),
    // ic13 (256 / 128@2x), ic14 (512 / 256@2x).
    const ENTRIES: &[(&[u8; 4], u32)] = &[
        (b"icp4", 16),
        (b"icp5", 32),
        (b"icp6", 64),
        (b"ic07", 128),
        (b"ic08", 256),
        (b"ic09", 512),
        (b"ic10", 1024),
        (b"ic11", 32),
        (b"ic12", 64),
        (b"ic13", 256),
        (b"ic14", 512),
    ];

    let mut chunks: Vec<(&[u8; 4], Vec<u8>)> = Vec::with_capacity(ENTRIES.len());
    let mut total_size: u32 = 8; // 4-byte magic + 4-byte total length
    for &(ostype, size) in ENTRIES {
        let png = render_pixmap(tree, size, size)
            .encode_png()
            .unwrap_or_else(|e| panic!("failed to encode {size}×{size} PNG for ICNS: {e}"));
        let chunk_len = u32::try_from(png.len() + 8).expect("ICNS PNG chunk size fits in u32");
        total_size = total_size
            .checked_add(chunk_len)
            .expect("ICNS total file size fits in u32");
        chunks.push((ostype, png));
    }

    let mut file = BufWriter::new(
        File::create(out_path)
            .unwrap_or_else(|e| panic!("failed to create {}: {e}", out_path.display())),
    );
    file.write_all(b"icns")
        .and_then(|()| file.write_all(&total_size.to_be_bytes()))
        .unwrap_or_else(|e| panic!("failed to write ICNS header: {e}"));
    for (ostype, png) in chunks {
        let chunk_len = u32::try_from(png.len() + 8).expect("chunk length fits in u32");
        file.write_all(ostype)
            .and_then(|()| file.write_all(&chunk_len.to_be_bytes()))
            .and_then(|()| file.write_all(&png))
            .unwrap_or_else(|e| panic!("failed to write ICNS entry: {e}"));
    }
    file.flush()
        .unwrap_or_else(|e| panic!("failed to flush ICNS: {e}"));
}
