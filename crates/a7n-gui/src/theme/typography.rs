// SPDX-License-Identifier: GPL-3.0-or-later

use std::sync::Arc;

use eframe::egui::{FontData, FontDefinitions, FontFamily, FontId, TextStyle};

/// Spacing / sizing tokens.
pub(crate) mod size {
    pub(crate) const CONTROL_H: f32 = 32.0;
    pub(crate) const CONTROL_H_SM: f32 = 26.0;
    pub(crate) const NAV_ITEM_H: f32 = 36.0;
    pub(crate) const SIDEBAR_W: f32 = 232.0;
    pub(crate) const CONTENT_MAX_W: f32 = 960.0;
    pub(crate) const ICON: f32 = 18.0;
    pub(crate) const ICON_LG: f32 = 20.0;
    pub(crate) const SECTION_GAP: f32 = 32.0;
    pub(crate) const ROW_GAP: f32 = 12.0;

    pub(crate) const R_KEYCAP: u8 = 3;
    pub(crate) const R_CONTROL: u8 = 4;
    pub(crate) const R_CARD: u8 = 6;
    pub(crate) const R_POPUP: u8 = 8;
    pub(crate) const R_PILL: u8 = 11;
}

/// Font family names registered by `install`.
pub(crate) mod family {
    pub(crate) const ICON: &str = "phosphor";
    pub(crate) const MEDIUM: &str = "inter-medium";
    pub(crate) const SEMIBOLD: &str = "inter-semibold";
}

pub(crate) fn font_regular(size: f32) -> FontId {
    FontId::new(size, FontFamily::Proportional)
}
pub(crate) fn font_medium(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(super::family::MEDIUM.into()))
}
pub(crate) fn font_semibold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(super::family::SEMIBOLD.into()))
}
pub(crate) fn font_icon(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(super::family::ICON.into()))
}
pub(crate) fn font_mono(size: f32) -> FontId {
    FontId::new(size, FontFamily::Monospace)
}

/// Custom text styles (use with `RichText::text_style`).
pub(crate) fn title_style() -> TextStyle {
    TextStyle::Name("title".into())
}
pub(crate) fn overline_style() -> TextStyle {
    TextStyle::Name("overline".into())
}

pub(super) fn fonts() -> FontDefinitions {
    let mut fonts = FontDefinitions::default();
    let mut add = |name: &str, bytes: &'static [u8]| {
        fonts
            .font_data
            .insert(name.to_owned(), Arc::new(FontData::from_static(bytes)));
    };
    add(
        "inter",
        include_bytes!("../../assets/fonts/Inter-Regular.ttf"),
    );
    add(
        super::family::MEDIUM,
        include_bytes!("../../assets/fonts/Inter-Medium.ttf"),
    );
    add(
        super::family::SEMIBOLD,
        include_bytes!("../../assets/fonts/Inter-SemiBold.ttf"),
    );
    add(
        "jbmono",
        include_bytes!("../../assets/fonts/JetBrainsMono-Regular.ttf"),
    );
    // Phosphor icon font. If your egui-phosphor version lacks `font_data()`, use
    // `egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Regular)` and
    // replace "phosphor" below with the key it inserts.
    fonts.font_data.insert(
        "phosphor".to_owned(),
        Arc::new(egui_phosphor::Variant::Regular.font_data()),
    );

    fonts.families.insert(
        FontFamily::Name(super::family::ICON.into()),
        vec!["phosphor".to_owned()],
    );

    let fam = |list: &[&str]| list.iter().map(ToString::to_string).collect::<Vec<_>>();
    fonts
        .families
        .insert(FontFamily::Proportional, fam(&["inter", "phosphor"]));
    fonts
        .families
        .insert(FontFamily::Monospace, fam(&["jbmono", "phosphor", "inter"]));
    fonts.families.insert(
        FontFamily::Name(super::family::MEDIUM.into()),
        fam(&[super::family::MEDIUM, "phosphor"]),
    );
    fonts.families.insert(
        FontFamily::Name(super::family::SEMIBOLD.into()),
        fam(&[super::family::SEMIBOLD, "phosphor"]),
    );
    fonts
}
