// SPDX-License-Identifier: GPL-3.0-or-later

use eframe::egui::{
    Context, CornerRadius, CursorIcon, Margin, Stroke, Style, TextStyle, Theme, ThemePreference,
    Visuals, epaint::Shadow, vec2,
};

use super::typography::fonts;
use super::{
    DARK, LIGHT, Palette, font_medium, font_mono, font_regular, font_semibold, overline_style,
    size, title_style,
};

/// Register fonts and both theme styles; follow the OS light/dark setting.
pub(crate) fn install(ctx: &Context) {
    ctx.set_fonts(fonts());
    ctx.set_style_of(Theme::Light, style(&LIGHT, false));
    ctx.set_style_of(Theme::Dark, style(&DARK, true));
    ctx.set_theme(ThemePreference::System);
}

fn style(palette: &Palette, dark: bool) -> Style {
    let mut style = Style {
        text_styles: [
            (TextStyle::Small, font_regular(12.0)),
            (TextStyle::Body, font_regular(14.0)),
            (TextStyle::Button, font_medium(14.0)),
            (TextStyle::Monospace, font_mono(13.0)),
            (TextStyle::Heading, font_semibold(16.0)),
            (title_style(), font_semibold(24.0)),
            (overline_style(), font_mono(11.0)),
        ]
        .into(),
        ..Style::default()
    };

    // --- Spacing ---------------------------------------------------------------------------
    let spacing = &mut style.spacing;
    spacing.item_spacing = vec2(8.0, 8.0);
    spacing.button_padding = vec2(14.0, 7.0);
    spacing.interact_size = vec2(32.0, size::CONTROL_H);
    spacing.icon_width = 16.0;
    spacing.icon_width_inner = 10.0;
    spacing.icon_spacing = 8.0;
    spacing.indent = 16.0;
    spacing.text_edit_width = 280.0;
    spacing.combo_width = 180.0;
    spacing.window_margin = Margin::same(24);
    spacing.menu_margin = Margin::same(6);
    spacing.scroll.bar_width = 8.0;

    style.interaction.selectable_labels = false; // labels are not text-selectable by default

    // --- Visuals ---------------------------------------------------------------------------
    let mut visuals = if dark {
        Visuals::dark()
    } else {
        Visuals::light()
    };
    let radius = CornerRadius::same(size::R_CONTROL);

    visuals.panel_fill = palette.bg;
    visuals.window_fill = palette.surface;
    visuals.window_stroke = Stroke::new(1.0, palette.border);
    visuals.extreme_bg_color = palette.input; // TextEdit background
    visuals.text_edit_bg_color = Some(palette.input);
    visuals.faint_bg_color = palette.hover; // striped rows
    visuals.code_bg_color = palette.hover;
    visuals.hyperlink_color = palette.accent_text;
    visuals.warn_fg_color = palette.warning;
    visuals.error_fg_color = palette.danger;
    visuals.override_text_color = None;

    visuals.window_corner_radius = CornerRadius::same(size::R_POPUP);
    visuals.menu_corner_radius = CornerRadius::same(size::R_CARD);
    let shadow = Shadow {
        offset: [0, 8],
        blur: 24,
        spread: 0,
        color: palette.shadow,
    };
    visuals.window_shadow = shadow;
    visuals.popup_shadow = shadow;

    visuals.selection.bg_fill = palette.accent_subtle;
    visuals.selection.stroke = Stroke::new(1.0, palette.accent_text);
    visuals.text_cursor.stroke = Stroke::new(2.0, palette.accent);
    visuals.interact_cursor = Some(CursorIcon::PointingHand);
    visuals.striped = false;
    visuals.indent_has_left_vline = false;

    let widgets = &mut visuals.widgets;
    // labels, separators, frames
    widgets.noninteractive.bg_fill = palette.surface;
    widgets.noninteractive.weak_bg_fill = palette.surface;
    widgets.noninteractive.bg_stroke = Stroke::new(1.0, palette.border);
    widgets.noninteractive.fg_stroke = Stroke::new(1.0, palette.text2);
    widgets.noninteractive.corner_radius = radius;
    // idle widgets (default egui buttons render as "secondary")
    widgets.inactive.bg_fill = palette.input; // checkbox box, slider rail
    widgets.inactive.weak_bg_fill = palette.surface; // button fill
    widgets.inactive.bg_stroke = Stroke::new(1.0, palette.border);
    widgets.inactive.fg_stroke = Stroke::new(1.0, palette.text);
    widgets.inactive.corner_radius = radius;
    widgets.inactive.expansion = 0.0;
    // hovered
    widgets.hovered.bg_fill = palette.hover;
    widgets.hovered.weak_bg_fill = palette.hover;
    widgets.hovered.bg_stroke = Stroke::new(1.0, palette.border_strong);
    widgets.hovered.fg_stroke = Stroke::new(1.0, palette.text);
    widgets.hovered.corner_radius = radius;
    widgets.hovered.expansion = 0.0;
    // pressed / focused text edit
    widgets.active.bg_fill = palette.pressed;
    widgets.active.weak_bg_fill = palette.pressed;
    widgets.active.bg_stroke = Stroke::new(1.0, palette.accent);
    widgets.active.fg_stroke = Stroke::new(1.0, palette.text);
    widgets.active.corner_radius = radius;
    widgets.active.expansion = 0.0;
    // open combo boxes / menus
    widgets.open.bg_fill = palette.hover;
    widgets.open.weak_bg_fill = palette.hover;
    widgets.open.bg_stroke = Stroke::new(1.0, palette.border_strong);
    widgets.open.fg_stroke = Stroke::new(1.0, palette.text);
    widgets.open.corner_radius = radius;

    style.visuals = visuals;
    style
}
