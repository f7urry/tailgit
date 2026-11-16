//! Apple Human Interface Guidelines styling for egui.
//!
//! - Interface font Inter (bundled; System/SF Pro selectable) and SF Mono with the macOS type ramp: 13pt body,
//!   11pt small, 10pt caption, 15pt headline.
//! - macOS semantic system colors for light and dark appearances
//!   (accent = systemBlue, labels, separators, window/control backgrounds).
//! - Control metrics: 6pt control corners, 10pt window/popover corners,
//!   soft layered shadows, 8pt spacing grid, 22pt min control height.
//! - Follows the system appearance (light/dark) automatically.

use std::collections::BTreeMap;
use std::sync::Arc;

use eframe::egui::{
    self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Margin, Shadow,
    Stroke, TextStyle, Theme, Vec2, Visuals,
};

/// macOS system colors (light, dark).
pub mod sys {
    use eframe::egui::Color32;

    const fn rgb(r: u8, g: u8, b: u8) -> Color32 {
        Color32::from_rgb(r, g, b)
    }
    const fn pick(dark: bool, l: Color32, d: Color32) -> Color32 {
        if dark { d } else { l }
    }

    pub fn blue(dark: bool) -> Color32 { pick(dark, rgb(0, 122, 255), rgb(10, 132, 255)) }
    pub fn green(dark: bool) -> Color32 { pick(dark, rgb(40, 167, 69), rgb(48, 209, 88)) }
    pub fn red(dark: bool) -> Color32 { pick(dark, rgb(255, 59, 48), rgb(255, 69, 58)) }
    pub fn orange(dark: bool) -> Color32 { pick(dark, rgb(255, 149, 0), rgb(255, 159, 10)) }
    pub fn yellow(dark: bool) -> Color32 { pick(dark, rgb(255, 204, 0), rgb(255, 214, 10)) }
    pub fn purple(dark: bool) -> Color32 { pick(dark, rgb(175, 82, 222), rgb(191, 90, 242)) }
    pub fn teal(dark: bool) -> Color32 { pick(dark, rgb(48, 176, 199), rgb(64, 200, 224)) }
    pub fn indigo(dark: bool) -> Color32 { pick(dark, rgb(88, 86, 214), rgb(94, 92, 230)) }
    pub fn gray(dark: bool) -> Color32 { pick(dark, rgb(142, 142, 147), rgb(152, 152, 157)) }
}

pub fn install(ctx: &egui::Context, settings: &crate::settings::Settings) {
    install_fonts(ctx, &settings.ui_font, settings.ui_font_weight);
    apply_font_size(ctx, settings.ui_font_size);
    apply_text_options(ctx, settings.text_hinting, settings.text_subpixel);
    apply_appearance(ctx, settings.appearance);
}

pub fn apply_text_options(ctx: &egui::Context, hinting: bool, subpixel: bool) {
    for theme in [Theme::Light, Theme::Dark] {
        ctx.style_mut_of(theme, |s| {
            s.visuals.text_options.font_hinting = hinting;
            s.visuals.text_options.subpixel_binning = subpixel;
        });
    }
}

pub fn apply_font_size(ctx: &egui::Context, size: f32) {
    ctx.set_style_of(Theme::Light, style(false, size));
    ctx.set_style_of(Theme::Dark, style(true, size));
}

/// Sorted family names of every installed font (plus the bundled/system entries).
pub fn system_families() -> &'static [String] {
    static FAMILIES: std::sync::OnceLock<Vec<String>> = std::sync::OnceLock::new();
    FAMILIES.get_or_init(|| {
        let mut db = fontdb::Database::new();
        db.load_system_fonts();
        let mut names: Vec<String> = db.faces().filter_map(|f| f.families.first().map(|(n, _)| n.clone())).collect();
        names.sort_by_key(|n| n.to_lowercase());
        names.dedup();
        names.retain(|n| !n.starts_with('.') && n != "Inter" && n != "System");
        names
    })
}

fn load_family(name: &str) -> Option<FontData> {
    let mut db = fontdb::Database::new();
    db.load_system_fonts();
    let q = fontdb::Query { families: &[fontdb::Family::Name(name)], ..Default::default() };
    let id = db.query(&q)?;
    db.with_face_data(id, |data, index| {
        let mut fd = FontData::from_owned(data.to_vec());
        fd.index = index;
        fd
    })
}

pub fn apply_appearance(ctx: &egui::Context, a: crate::settings::Appearance) {
    use crate::settings::Appearance;
    ctx.set_theme(match a {
        Appearance::System => egui::ThemePreference::System,
        Appearance::Light => egui::ThemePreference::Light,
        Appearance::Dark => egui::ThemePreference::Dark,
    });
}

fn load(path: &str) -> Option<FontData> {
    std::fs::read(path).ok().map(FontData::from_owned)
}

pub fn install_fonts(ctx: &egui::Context, ui_font: &str, weight: f32) {
    let mut fonts = FontDefinitions::default();
    // Inter (SIL OFL 1.1) is bundled; "System" is SF Pro; anything else is an installed family.
    let system = || load("/System/Library/Fonts/SFNS.ttf").or_else(|| load("/System/Library/Fonts/HelveticaNeue.ttc"));
    let sans = match ui_font {
        "Inter" => Some(FontData::from_static(include_bytes!("../assets/Inter.ttf"))),
        "System" => system(),
        name => load_family(name),
    }
    .or_else(|| Some(FontData::from_static(include_bytes!("../assets/Inter.ttf"))))
    .map(|mut f| {
        // Variable fonts (Inter, SF) honor the weight axis; static fonts ignore it.
        f.tweak.coords = egui::epaint::text::VariationCoords::new([("wght", weight.clamp(100.0, 900.0))]);
        f
    });
    let mono = load("/System/Library/Fonts/SFNSMono.ttf")
        .or_else(|| load("/System/Library/Fonts/Menlo.ttc"));
    if let Some(f) = sans {
        fonts.font_data.insert("system".into(), Arc::new(f));
        fonts.families.entry(FontFamily::Proportional).or_default().insert(0, "system".into());
    }
    if let Some(f) = mono {
        fonts.font_data.insert("system-mono".into(), Arc::new(f));
        fonts.families.entry(FontFamily::Monospace).or_default().insert(0, "system-mono".into());
    }
    // Font Awesome 5 Free Solid (SIL OFL 1.1): icon glyphs in the Private Use Area.
    fonts.font_data.insert(
        "fa".into(),
        Arc::new(FontData::from_static(include_bytes!("../assets/fa-solid-900.ttf"))),
    );
    // First in the chain: SF Pro also has legacy glyphs in the PUA (e.g. U+F002
    // is an "fl" ligature), which would otherwise shadow the icons. Font Awesome
    // only covers PUA codepoints, so regular text still falls through to SF.
    for fam in [FontFamily::Proportional, FontFamily::Monospace] {
        fonts.families.entry(fam).or_default().insert(0, "fa".into());
    }
    ctx.set_fonts(fonts);
}

fn style(dark: bool, size: f32) -> egui::Style {
    let mut s = egui::Style {
        // macOS type ramp (points).
        text_styles: BTreeMap::from([
            (TextStyle::Heading, FontId::proportional(size + 2.0)),
            (TextStyle::Body, FontId::proportional(size)),
            (TextStyle::Button, FontId::proportional(size)),
            (TextStyle::Small, FontId::proportional((size - 2.0).max(8.0))),
            (TextStyle::Monospace, FontId::monospace(size - 1.0)),
        ]),
        ..Default::default()
    };

    // 8pt grid.
    let sp = &mut s.spacing;
    sp.item_spacing = Vec2::new(8.0, 6.0);
    sp.button_padding = Vec2::new(10.0, 3.0);
    sp.interact_size = Vec2::new(24.0, 22.0);
    sp.window_margin = Margin::same(16);
    sp.menu_margin = Margin::same(10);
    sp.indent = 16.0;
    sp.icon_spacing = 6.0;

    s.visuals = visuals(dark);
    s
}

fn visuals(dark: bool) -> Visuals {
    let mut v = if dark { Visuals::dark() } else { Visuals::light() };
    let gray = |g: u8| Color32::from_gray(g);
    let accent = sys::blue(dark);

    // Semantic backgrounds: windowBackground / controlBackground / textBackground.
    let (window_bg, panel_bg, content_bg, control, control_hover, control_press) = if dark {
        (gray(36), gray(30), gray(30), gray(64), gray(76), gray(88))
    } else {
        (gray(246), gray(236), Color32::WHITE, Color32::WHITE, gray(246), gray(228))
    };
    let label = if dark { Color32::from_white_alpha(217) } else { Color32::from_black_alpha(217) };
    let separator = if dark { Color32::from_white_alpha(28) } else { Color32::from_black_alpha(26) };
    let control_border = if dark { Color32::from_white_alpha(20) } else { Color32::from_black_alpha(38) };

    v.panel_fill = panel_bg;
    v.window_fill = window_bg;
    v.extreme_bg_color = content_bg;
    v.text_edit_bg_color = Some(content_bg);
    v.faint_bg_color = if dark { Color32::from_white_alpha(6) } else { Color32::from_black_alpha(8) };
    v.code_bg_color = if dark { gray(44) } else { gray(240) };
    v.hyperlink_color = accent;
    v.warn_fg_color = sys::orange(dark);
    v.error_fg_color = sys::red(dark);
    v.weak_text_alpha = 0.55;

    v.selection.bg_fill = if dark {
        Color32::from_rgba_unmultiplied(10, 132, 255, 140)
    } else {
        Color32::from_rgba_unmultiplied(0, 122, 255, 60)
    };
    v.selection.stroke = Stroke::new(1.0, if dark { Color32::WHITE } else { accent });

    // Windows, popovers and menus: rounded, translucent-feeling, soft shadow.
    v.window_corner_radius = CornerRadius::same(10);
    v.menu_corner_radius = CornerRadius::same(8);
    v.window_stroke = Stroke::new(1.0, separator);
    let shadow_alpha = if dark { 110 } else { 45 };
    v.window_shadow = Shadow { offset: [0, 8], blur: 28, spread: 0, color: Color32::from_black_alpha(shadow_alpha) };
    v.popup_shadow = Shadow { offset: [0, 4], blur: 14, spread: 0, color: Color32::from_black_alpha(shadow_alpha) };
    v.window_highlight_topmost = false;

    let r = CornerRadius::same(6);
    let w = &mut v.widgets;
    w.noninteractive.bg_fill = window_bg;
    w.noninteractive.weak_bg_fill = window_bg;
    w.noninteractive.bg_stroke = Stroke::new(1.0, separator);
    w.noninteractive.fg_stroke = Stroke::new(1.0, label);
    w.noninteractive.corner_radius = r;

    for (wv, fill) in [
        (&mut w.inactive, control),
        (&mut w.hovered, control_hover),
        (&mut w.active, control_press),
        (&mut w.open, control_press),
    ] {
        wv.bg_fill = fill;
        wv.weak_bg_fill = fill;
        wv.bg_stroke = Stroke::new(1.0, control_border);
        wv.fg_stroke = Stroke::new(1.0, label);
        wv.corner_radius = r;
        wv.expansion = 0.0;
    }
    // Press feedback is instant and subtle (HIG: respond on press).
    w.active.fg_stroke = Stroke::new(1.0, if dark { Color32::WHITE } else { Color32::BLACK });
    w.hovered.bg_stroke = Stroke::new(1.0, control_border);

    v.striped = true;
    v.indent_has_left_vline = false;
    v.collapsing_header_frame = false;
    v.slider_trailing_fill = true;
    v.interact_cursor = None; // macOS uses the arrow cursor on controls
    v
}

/// Window background showing between islands.
pub fn gap_color(dark: bool) -> Color32 {
    if dark { Color32::from_gray(22) } else { Color32::from_gray(232) }
}

/// Fill of an island (rounded content card).
pub fn island_color(dark: bool) -> Color32 {
    if dark { Color32::from_gray(36) } else { Color32::WHITE }
}

/// Borderless rounded card with a gap around it (IntelliJ "islands" layout).
/// `outer` is the gap on each side: (left, right, top, bottom).
pub fn island(dark: bool, outer: (i8, i8, i8, i8)) -> egui::Frame {
    egui::Frame::new()
        .fill(island_color(dark))
        .corner_radius(CornerRadius::same(10))
        .inner_margin(Margin::same(8))
        .outer_margin(Margin { left: outer.0, right: outer.1, top: outer.2, bottom: outer.3 })
}

/// Frame for chrome bars (toolbar, status bar) that sit directly on the gap.
pub fn chrome(dark: bool) -> egui::Frame {
    egui::Frame::new().fill(gap_color(dark)).inner_margin(Margin::symmetric(8, 2))
}
