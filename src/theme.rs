//! The kjk design system's tokens: colors, spacing, corner radii and text
//! styles. No other module writes a color, a font size or a radius as a
//! literal. The names follow the design system's token names.

use eframe::egui::{
    self, Color32, CornerRadius, FontFamily, FontId, Margin, Shadow, Stroke, TextStyle,
};

pub const GROUND: Color32 = Color32::from_rgb(0x12, 0x12, 0x12);
pub const SURFACE: Color32 = Color32::from_rgb(0x1a, 0x1b, 0x1a);
pub const SURFACE_RAISED: Color32 = Color32::from_rgb(0x22, 0x24, 0x23);
pub const LINE: Color32 = Color32::from_rgb(0x2a, 0x2c, 0x2b);
pub const CONTROL_BORDER: Color32 = Color32::from_rgb(0x6b, 0x70, 0x6d);
pub const INK: Color32 = Color32::from_rgb(0xe6, 0xe9, 0xe7);
pub const INK_BRIGHT: Color32 = Color32::from_rgb(0xf7, 0xf8, 0xf7);
pub const INK_MUTED: Color32 = Color32::from_rgb(0xa3, 0xa8, 0xa5);
pub const INK_FAINT: Color32 = Color32::from_rgb(0x86, 0x8b, 0x88);
pub const ON_INK: Color32 = Color32::from_rgb(0x12, 0x12, 0x12);
pub const OK: Color32 = Color32::from_rgb(0x73, 0xd1, 0x85);
pub const WARN: Color32 = Color32::from_rgb(0xf2, 0xad, 0x4d);
pub const ERROR: Color32 = Color32::from_rgb(0xf2, 0x73, 0x6b);

pub const SPACE_1: f32 = 4.0;
pub const SPACE_2: f32 = 8.0;
pub const SPACE_3: f32 = 12.0;
pub const SPACE_4: f32 = 16.0;
pub const SPACE_6: f32 = 24.0;
pub const SPACE_8: f32 = 32.0;

pub const RADIUS_SM: u8 = 4;
pub const RADIUS_MD: u8 = 6;
pub const RADIUS_LG: u8 = 10;

/// A hairline, a control's outline and the progress line.
pub const HAIRLINE: f32 = 1.0;
/// The keyboard focus outline: this wide, this far outside the control.
pub const FOCUS_WIDTH: f32 = 2.0;
pub const FOCUS_GAP: f32 = 2.0;
/// The status dot's diameter.
pub const DOT: f32 = 8.0;
/// The mark's side in the top bar.
pub const MARK: f32 = 28.0;
/// The progress line's height.
pub const PROGRESS: f32 = 2.0;
/// The chevron after the version title.
pub const CHEVRON: f32 = 10.0;

/// The version menu's width.
pub const MENU_WIDTH: f32 = 360.0;
/// The most height the version menu's rows take before they scroll.
pub const MENU_HEIGHT: f32 = 320.0;
/// A menu row's height.
pub const ROW: f32 = 32.0;

/// The window's size when it opens, and the smallest it can be made.
pub const WINDOW: [f32; 2] = [960.0, 600.0];
pub const WINDOW_MIN: [f32; 2] = [800.0, 520.0];

/// The text styles of the design system.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Text {
    /// The version number: 40, Jost SemiBold.
    Display,
    /// Page and dialog titles: 22, Jost SemiBold.
    Title,
    /// Row names: 15, Geist SemiBold.
    Heading,
    /// Sentences: 14, Geist Regular.
    Body,
    /// Tabs, status words and section labels: 12, Geist Medium.
    Label,
    /// Paths and tags: 12, Geist Mono Regular.
    Mono,
    /// "kjk-engine" in the top bar: 18, Geist Mono Medium.
    Wordmark,
}

/// The design system's fonts, embedded in the program: (family name, file).
/// Their licence texts are `assets/fonts/OFL-Jost.txt` and `OFL-Geist.txt`.
const FONTS: [(&str, &[u8]); 6] = [
    (
        JOST_SEMIBOLD,
        include_bytes!("../assets/fonts/Jost-SemiBold.ttf"),
    ),
    (
        GEIST_REGULAR,
        include_bytes!("../assets/fonts/Geist-Regular.ttf"),
    ),
    (
        GEIST_MEDIUM,
        include_bytes!("../assets/fonts/Geist-Medium.ttf"),
    ),
    (
        GEIST_SEMIBOLD,
        include_bytes!("../assets/fonts/Geist-SemiBold.ttf"),
    ),
    (
        GEIST_MONO_REGULAR,
        include_bytes!("../assets/fonts/GeistMono-Regular.ttf"),
    ),
    (
        GEIST_MONO_MEDIUM,
        include_bytes!("../assets/fonts/GeistMono-Medium.ttf"),
    ),
];
const JOST_SEMIBOLD: &str = "Jost SemiBold";
const GEIST_REGULAR: &str = "Geist Regular";
const GEIST_MEDIUM: &str = "Geist Medium";
const GEIST_SEMIBOLD: &str = "Geist SemiBold";
const GEIST_MONO_REGULAR: &str = "Geist Mono Regular";
const GEIST_MONO_MEDIUM: &str = "Geist Mono Medium";

/// The size and font of a text style. Body text is egui's proportional
/// family and mono text its monospace family, both set to Geist, so plain
/// egui widgets use them too.
pub fn font(text: Text) -> FontId {
    let named = |name: &str| FontFamily::Name(name.into());
    let (size, family) = match text {
        Text::Display => (40.0, named(JOST_SEMIBOLD)),
        Text::Title => (22.0, named(JOST_SEMIBOLD)),
        Text::Heading => (15.0, named(GEIST_SEMIBOLD)),
        Text::Body => (14.0, FontFamily::Proportional),
        Text::Label => (12.0, named(GEIST_MEDIUM)),
        Text::Mono => (12.0, FontFamily::Monospace),
        Text::Wordmark => (18.0, named(GEIST_MONO_MEDIUM)),
    };
    FontId::new(size, family)
}

/// Sets the fonts, the colors and the spacing of the whole window.
pub fn install(ctx: &egui::Context) {
    install_fonts(ctx);
    ctx.all_styles_mut(|style| {
        style.text_styles = [
            (TextStyle::Heading, font(Text::Title)),
            (TextStyle::Body, font(Text::Body)),
            (TextStyle::Button, font(Text::Body)),
            (TextStyle::Small, font(Text::Label)),
            (TextStyle::Monospace, font(Text::Mono)),
        ]
        .into();
        style.spacing.item_spacing = egui::vec2(SPACE_2, SPACE_2);
        style.spacing.button_padding = egui::vec2(SPACE_4, SPACE_2);
        style.spacing.window_margin = Margin::same(SPACE_6 as i8);
        style.spacing.menu_margin = Margin::same(SPACE_2 as i8);
        visuals(&mut style.visuals);
    });
}

/// The one place that loads fonts. Each font of the design system is its
/// own family, followed by egui's own fonts for the signs Geist and Jost do
/// not have. egui's proportional family starts with Geist Regular and its
/// monospace family with Geist Mono Regular.
fn install_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    let fallback = fonts
        .families
        .get(&FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();
    for (name, bytes) in FONTS {
        fonts.font_data.insert(
            name.to_string(),
            std::sync::Arc::new(egui::FontData::from_static(bytes)),
        );
        let mut family = vec![name.to_string()];
        family.extend(fallback.iter().cloned());
        fonts.families.insert(FontFamily::Name(name.into()), family);
    }
    for (family, first) in [
        (FontFamily::Proportional, GEIST_REGULAR),
        (FontFamily::Monospace, GEIST_MONO_REGULAR),
    ] {
        fonts
            .families
            .entry(family)
            .or_default()
            .insert(0, first.to_string());
    }
    ctx.set_fonts(fonts);
}

fn visuals(v: &mut egui::Visuals) {
    *v = egui::Visuals::dark();
    let stroke = |color| Stroke::new(HAIRLINE, color);
    v.panel_fill = GROUND;
    v.window_fill = SURFACE_RAISED;
    v.window_stroke = stroke(LINE);
    v.window_corner_radius = CornerRadius::same(RADIUS_LG);
    v.menu_corner_radius = CornerRadius::same(RADIUS_LG);
    v.window_shadow = Shadow::NONE;
    v.popup_shadow = Shadow::NONE;
    v.faint_bg_color = SURFACE;
    v.extreme_bg_color = SURFACE;
    v.code_bg_color = SURFACE;
    v.hyperlink_color = INK;
    v.warn_fg_color = WARN;
    v.error_fg_color = ERROR;
    v.weak_text_color = Some(INK_MUTED);
    v.selection.bg_fill = SURFACE_RAISED;
    v.selection.stroke = Stroke::new(FOCUS_WIDTH, INK);

    let w = &mut v.widgets;
    w.noninteractive.bg_fill = GROUND;
    w.noninteractive.weak_bg_fill = GROUND;
    w.noninteractive.bg_stroke = stroke(LINE);
    w.noninteractive.fg_stroke = stroke(INK);
    w.noninteractive.corner_radius = CornerRadius::same(RADIUS_SM);
    for (state, fill, border, ink) in [
        (&mut w.inactive, Color32::TRANSPARENT, CONTROL_BORDER, INK),
        (&mut w.hovered, SURFACE_RAISED, INK, INK_BRIGHT),
        (&mut w.active, LINE, INK, INK_BRIGHT),
        (&mut w.open, SURFACE_RAISED, INK, INK_BRIGHT),
    ] {
        state.weak_bg_fill = fill;
        state.bg_fill = SURFACE_RAISED;
        state.bg_stroke = stroke(border);
        state.fg_stroke = stroke(ink);
        state.corner_radius = CornerRadius::same(RADIUS_MD);
        state.expansion = 0.0;
    }
}

/// The kjk mark, as a texture for the top bar.
pub fn mark(ctx: &egui::Context) -> Option<egui::TextureHandle> {
    let bytes = include_bytes!("../assets/kjk-mark.png");
    let picture = image::load_from_memory_with_format(bytes, image::ImageFormat::Png).ok()?;
    let rgba = picture.to_rgba8();
    let size = [rgba.width() as usize, rgba.height() as usize];
    let image = egui::ColorImage::from_rgba_unmultiplied(size, rgba.as_raw());
    Some(ctx.load_texture("kjk-mark", image, egui::TextureOptions::LINEAR))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_styles_keep_the_design_system_sizes() {
        let sizes = [
            (Text::Display, 40.0),
            (Text::Title, 22.0),
            (Text::Heading, 15.0),
            (Text::Body, 14.0),
            (Text::Label, 12.0),
            (Text::Mono, 12.0),
            (Text::Wordmark, 18.0),
        ];
        for (text, size) in sizes {
            assert_eq!(font(text).size, size, "{text:?}");
        }
        assert_eq!(font(Text::Mono).family, FontFamily::Monospace);
    }
}
