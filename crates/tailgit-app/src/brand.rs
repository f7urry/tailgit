//! App icon: crops the artwork to its squircle and masks the corners transparent.

use std::sync::OnceLock;

use eframe::egui;

const SRC: &[u8] = include_bytes!("../assets/icon.png");

/// (size, RGBA) of the masked icon.
pub fn icon_rgba() -> &'static (u32, Vec<u8>) {
    static ICON: OnceLock<(u32, Vec<u8>)> = OnceLock::new();
    ICON.get_or_init(|| {
        let img = image::load_from_memory(SRC).expect("icon").to_rgba8();
        let (w, h) = img.dimensions();
        // Bounds of the dark squircle (ignore the light margin and its shadow).
        let (mut x0, mut y0, mut x1, mut y1) = (w, h, 0, 0);
        for (x, y, p) in img.enumerate_pixels() {
            let lum = (p[0] as u32 + p[1] as u32 + p[2] as u32) / 3;
            if lum < 120 {
                x0 = x0.min(x);
                y0 = y0.min(y);
                x1 = x1.max(x);
                y1 = y1.max(y);
            }
        }
        let side = (x1 - x0).max(y1 - y0) + 1;
        let crop = image::imageops::crop_imm(&img, x0, y0, side, side).to_image();
        let n = 1024u32;
        let mut out = image::imageops::resize(&crop, n, n, image::imageops::FilterType::Lanczos3);
        // Superellipse mask (n = 5) with 1px anti-aliased edge.
        let r = n as f32 / 2.0;
        for (x, y, p) in out.enumerate_pixels_mut() {
            let dx = ((x as f32 + 0.5 - r) / r).abs();
            let dy = ((y as f32 + 0.5 - r) / r).abs();
            let d = (dx.powi(5) + dy.powi(5)).powf(0.2); // 1.0 on the edge
            let a = ((1.0 - d) * r + 0.5).clamp(0.0, 1.0);
            p[3] = (p[3] as f32 * a) as u8;
        }
        (n, out.into_raw())
    })
}

pub fn texture(ctx: &egui::Context) -> egui::TextureHandle {
    let id = egui::Id::unique("brand_icon_tex");
    if let Some(t) = ctx.data(|d| d.get_temp::<egui::TextureHandle>(id)) {
        return t;
    }
    let (n, rgba) = icon_rgba();
    let img = egui::ColorImage::from_rgba_unmultiplied([*n as usize, *n as usize], rgba);
    let t = ctx.load_texture("brand_icon", img, egui::TextureOptions::LINEAR);
    ctx.data_mut(|d| d.insert_temp(id, t.clone()));
    t
}

/// Writes a macOS-grid app icon: 1024px transparent canvas with the
/// squircle at 824px, centered (Apple's icon template proportions).
pub fn export_png(path: &std::path::Path) -> anyhow::Result<()> {
    let (n, rgba) = icon_rgba();
    let src = image::RgbaImage::from_raw(*n, *n, rgba.clone()).expect("icon buffer");
    let body = image::imageops::resize(&src, 824, 824, image::imageops::FilterType::Lanczos3);
    let mut canvas = image::RgbaImage::new(1024, 1024);
    image::imageops::overlay(&mut canvas, &body, 100, 92);
    canvas.save(path)?;
    Ok(())
}
