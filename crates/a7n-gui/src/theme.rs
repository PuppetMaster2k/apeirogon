// SPDX-License-Identifier: GPL-3.0-or-later
//! Apeirogon UI theme for egui.
//!
//! Target: egui / eframe 0.36 (checked with egui 0.36.2 + egui-phosphor 0.14).
//!         Works back to ~0.32 with small renames (e.g. `ctx.theme()`).
//! Crates:   egui-phosphor = "0.14", no other deps.
//! Assets:   assets/fonts/Inter-Regular.ttf, Inter-Medium.ttf, Inter-SemiBold.ttf,
//!           assets/fonts/JetBrainsMono-Regular.ttf   (static TTFs, not the variable fonts)
//!
//! Install with `theme::install(&cc.egui_ctx)` in `App::new`.
//! Helpers paint navigation, buttons, inputs, settings and cards with the palette.

use std::sync::Arc;

use eframe::egui;
use eframe::egui::{
    Align, Align2, Color32, Context, CornerRadius, CursorIcon, FontData, FontDefinitions,
    FontFamily, FontId, Frame, Layout, Margin, Response, RichText, Sense, Stroke, StrokeKind,
    Style, TextEdit, TextStyle, Theme, ThemePreference, Ui, Vec2, Visuals, WidgetInfo, WidgetText,
    WidgetType, epaint::Shadow, vec2,
};
use egui_phosphor::regular as ph;

// ---------------------------------------------------------------------------------------------
// Tokens
// ---------------------------------------------------------------------------------------------

const fn hex(rgb: u32) -> Color32 {
    let [_, red, green, blue] = rgb.to_be_bytes();
    Color32::from_rgb(red, green, blue)
}

/// All color tokens. Names match the design canvas ("Foundations" artboard).
#[derive(Clone, Copy, Debug)]
pub(crate) struct Palette {
    pub(crate) bg: Color32,
    pub(crate) sidebar: Color32,
    pub(crate) surface: Color32,
    pub(crate) input: Color32,
    pub(crate) hover: Color32,
    pub(crate) pressed: Color32,
    pub(crate) border: Color32,
    pub(crate) border_strong: Color32,
    pub(crate) text: Color32,
    pub(crate) text2: Color32,
    pub(crate) text3: Color32,
    pub(crate) accent: Color32,
    pub(crate) accent_hover: Color32,
    pub(crate) accent_press: Color32,
    pub(crate) accent_subtle: Color32,
    pub(crate) accent_text: Color32,
    pub(crate) on_accent: Color32,
    pub(crate) success: Color32,
    pub(crate) success_subtle: Color32,
    pub(crate) warning: Color32,
    pub(crate) warning_subtle: Color32,
    pub(crate) danger: Color32,
    pub(crate) danger_subtle: Color32,
    pub(crate) focus: Color32,
    pub(crate) shadow: Color32,
}

pub(crate) const LIGHT: Palette = Palette {
    bg: hex(0x00F4_F5F7),
    sidebar: hex(0x00EC_EEF2),
    surface: hex(0x00FF_FFFF),
    input: hex(0x00FF_FFFF),
    hover: hex(0x00EE_F0F3),
    pressed: hex(0x00E2_E5EA),
    border: hex(0x00DC_E0E6),
    border_strong: hex(0x00C2_C8D1),
    text: hex(0x0012_151A),
    text2: hex(0x0047_5060),
    text3: hex(0x005F_6878),
    accent: hex(0x002F_5BEA),
    accent_hover: hex(0x0024_49C4),
    accent_press: hex(0x001D_3BA3),
    accent_subtle: hex(0x00E7_EDFD),
    accent_text: hex(0x0024_49C4),
    on_accent: hex(0x00FF_FFFF),
    success: hex(0x0017_7245),
    success_subtle: hex(0x00E2_F3E9),
    warning: hex(0x00A3_540A),
    warning_subtle: hex(0x00FB_EFDF),
    danger: hex(0x00C4_2B26),
    danger_subtle: hex(0x00FB_E6E5),
    focus: Color32::from_rgba_premultiplied(19, 36, 94, 102), // #2F5BEA @ 40 %
    shadow: Color32::from_black_alpha(30),
};

pub(crate) const DARK: Palette = Palette {
    bg: hex(0x000E_1116),
    sidebar: hex(0x0011_151B),
    surface: hex(0x0017_1B22),
    input: hex(0x000F_1318),
    hover: hex(0x0022_2832),
    pressed: hex(0x002A_313C),
    border: hex(0x002A_303A),
    border_strong: hex(0x003A_4250),
    text: hex(0x00E6_E9EE),
    text2: hex(0x00A8_B0BC),
    text3: hex(0x008A_93A1),
    accent: hex(0x0070_88FF),
    accent_hover: hex(0x008C_9FFF),
    accent_press: hex(0x005A_73F0),
    accent_subtle: hex(0x001C_2443),
    accent_text: hex(0x009A_ABFF),
    on_accent: hex(0x000A_0E1C),
    success: hex(0x0043_C77F),
    success_subtle: hex(0x0013_291D),
    warning: hex(0x00E9_A23B),
    warning_subtle: hex(0x002D_2112),
    danger: hex(0x00F0_675E),
    danger_subtle: hex(0x0033_1715),
    focus: Color32::from_rgba_premultiplied(50, 61, 115, 115), // #7088FF @ 45 %
    shadow: Color32::from_black_alpha(128),
};

impl Palette {
    /// The palette matching the context's current theme.
    pub(crate) fn of(ctx: &Context) -> &'static Palette {
        if ctx.theme() == Theme::Dark {
            &DARK
        } else {
            &LIGHT
        }
    }
}

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
    FontId::new(size, FontFamily::Name(family::MEDIUM.into()))
}
pub(crate) fn font_semibold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(family::SEMIBOLD.into()))
}
pub(crate) fn font_icon(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(family::ICON.into()))
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

// ---------------------------------------------------------------------------------------------
// Install
// ---------------------------------------------------------------------------------------------

/// Register fonts and both theme styles; follow the OS light/dark setting.
pub(crate) fn install(ctx: &Context) {
    ctx.set_fonts(fonts());
    ctx.set_style_of(Theme::Light, style(&LIGHT, false));
    ctx.set_style_of(Theme::Dark, style(&DARK, true));
    ctx.set_theme(ThemePreference::System);
}

fn fonts() -> FontDefinitions {
    let mut fonts = FontDefinitions::default();
    let mut add = |name: &str, bytes: &'static [u8]| {
        fonts
            .font_data
            .insert(name.to_owned(), Arc::new(FontData::from_static(bytes)));
    };
    add("inter", include_bytes!("../assets/fonts/Inter-Regular.ttf"));
    add(
        family::MEDIUM,
        include_bytes!("../assets/fonts/Inter-Medium.ttf"),
    );
    add(
        family::SEMIBOLD,
        include_bytes!("../assets/fonts/Inter-SemiBold.ttf"),
    );
    add(
        "jbmono",
        include_bytes!("../assets/fonts/JetBrainsMono-Regular.ttf"),
    );
    // Phosphor icon font. If your egui-phosphor version lacks `font_data()`, use
    // `egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Regular)` and
    // replace "phosphor" below with the key it inserts.
    fonts.font_data.insert(
        "phosphor".to_owned(),
        Arc::new(egui_phosphor::Variant::Regular.font_data()),
    );

    fonts.families.insert(
        FontFamily::Name(family::ICON.into()),
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
        FontFamily::Name(family::MEDIUM.into()),
        fam(&[family::MEDIUM, "phosphor"]),
    );
    fonts.families.insert(
        FontFamily::Name(family::SEMIBOLD.into()),
        fam(&[family::SEMIBOLD, "phosphor"]),
    );
    fonts
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

// ---------------------------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum ButtonKind {
    /// One per view: the main action (Save, Show pairing code, Start daemon).
    Primary,
    /// Everything else that is a real action.
    Secondary,
    /// Toolbars, row actions, Cancel.
    Ghost,
    /// Destructive row actions (Unpair) — quiet until hovered.
    DangerQuiet,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum ButtonSize {
    Md,
    Sm,
}

struct Paint {
    fill: Color32,
    stroke: Color32,
    text: Color32,
}

fn button_paint(p: &Palette, kind: ButtonKind, hovered: bool, pressed: bool) -> Paint {
    use ButtonKind::{DangerQuiet, Ghost, Primary, Secondary};
    let t = Color32::TRANSPARENT;
    match (kind, pressed, hovered) {
        (Primary, true, _) => Paint {
            fill: p.accent_press,
            stroke: p.accent_press,
            text: p.on_accent,
        },
        (Primary, _, true) => Paint {
            fill: p.accent_hover,
            stroke: p.accent_hover,
            text: p.on_accent,
        },
        (Primary, _, _) => Paint {
            fill: p.accent,
            stroke: p.accent,
            text: p.on_accent,
        },
        (Secondary, true, _) => Paint {
            fill: p.pressed,
            stroke: p.border_strong,
            text: p.text,
        },
        (Secondary, _, true) => Paint {
            fill: p.hover,
            stroke: p.border_strong,
            text: p.text,
        },
        (Secondary, _, _) => Paint {
            fill: p.surface,
            stroke: p.border,
            text: p.text,
        },
        (Ghost, true, _) => Paint {
            fill: p.pressed,
            stroke: t,
            text: p.text,
        },
        (Ghost, _, true) => Paint {
            fill: p.hover,
            stroke: t,
            text: p.text,
        },
        (Ghost, _, _) => Paint {
            fill: t,
            stroke: t,
            text: p.text,
        },
        (DangerQuiet, true, _) => Paint {
            fill: p.danger_subtle,
            stroke: p.danger,
            text: p.danger,
        },
        (DangerQuiet, _, true) => Paint {
            fill: p.danger_subtle,
            stroke: t,
            text: p.danger,
        },
        (DangerQuiet, _, _) => Paint {
            fill: t,
            stroke: t,
            text: p.danger,
        },
    }
}

fn focus_ring(ui: &Ui, rect: egui::Rect, radius: u8) {
    let p = Palette::of(ui.ctx());
    ui.painter().rect_stroke(
        rect.expand(1.5),
        CornerRadius::same(radius + 2),
        Stroke::new(3.0, p.focus),
        StrokeKind::Outside,
    );
}

fn fade(c: Color32, enabled: bool) -> Color32 {
    if enabled { c } else { c.gamma_multiply(0.45) }
}

/// Text button with optional leading Phosphor icon.
///
/// ```ignore
/// if theme::button(ui, ButtonKind::Primary, ButtonSize::Md, Some(ph::FLOPPY_DISK), "Save changes").clicked() { … }
/// ```
pub(crate) fn button(
    ui: &mut Ui,
    kind: ButtonKind,
    sz: ButtonSize,
    icon: Option<&str>,
    label: &str,
) -> Response {
    let p = Palette::of(ui.ctx());
    let (h, font, icon_size, pad_x) = match sz {
        ButtonSize::Md => (size::CONTROL_H, font_medium(14.0), size::ICON, 14.0),
        ButtonSize::Sm => (size::CONTROL_H_SM, font_medium(12.0), 14.0, 10.0),
    };
    let painter = ui.painter().clone();
    let text = painter.layout_no_wrap(label.to_owned(), font, Color32::PLACEHOLDER);
    let icon_g = icon
        .map(|i| painter.layout_no_wrap(i.to_owned(), font_icon(icon_size), Color32::PLACEHOLDER));
    let gap = 8.0;
    let icon_w = icon_g.as_ref().map_or(0.0, |g| g.size().x + gap);
    let pad_left = if icon.is_some() { pad_x - 2.0 } else { pad_x };
    let width = pad_left + icon_w + text.size().x + pad_x;

    let (rect, resp) = ui.allocate_exact_size(vec2(width, h), Sense::click());
    resp.widget_info(|| WidgetInfo::labeled(WidgetType::Button, ui.is_enabled(), label));

    if ui.is_rect_visible(rect) {
        let enabled = ui.is_enabled();
        let pressed = enabled && resp.is_pointer_button_down_on();
        let hovered = enabled && resp.hovered();
        let c = button_paint(p, kind, hovered, pressed);
        painter.rect(
            rect,
            CornerRadius::same(size::R_CONTROL),
            fade(c.fill, enabled),
            Stroke::new(1.0, fade(c.stroke, enabled)),
            StrokeKind::Inside,
        );
        let mut x = rect.left() + pad_left;
        if let Some(g) = icon_g {
            let pos = egui::pos2(x, rect.center().y - g.size().y / 2.0);
            x += g.size().x + gap;
            painter.galley(pos, g, fade(c.text, enabled));
        }
        let pos = egui::pos2(x, rect.center().y - text.size().y / 2.0);
        painter.galley(pos, text, fade(c.text, enabled));
        if resp.has_focus() {
            focus_ring(ui, rect, size::R_CONTROL);
        }
    }
    if ui.is_enabled() {
        resp.on_hover_cursor(CursorIcon::PointingHand)
    } else {
        resp
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum IconButtonKind {
    Ghost,
    Bordered,
    Danger,
}

/// Square icon-only button. Always pass a tooltip: it is also the accessible label.
pub(crate) fn icon_button(
    ui: &mut Ui,
    kind: IconButtonKind,
    icon: &str,
    tooltip: &str,
    selected: bool,
) -> Response {
    let p = Palette::of(ui.ctx());
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(size::CONTROL_H), Sense::click());
    resp.widget_info(|| {
        WidgetInfo::selected(WidgetType::Button, ui.is_enabled(), selected, tooltip)
    });
    if ui.is_rect_visible(rect) {
        let enabled = ui.is_enabled();
        let hovered = enabled && resp.hovered();
        let pressed = enabled && resp.is_pointer_button_down_on();
        let (mut fill, mut stroke, mut fg) = match kind {
            IconButtonKind::Ghost => (Color32::TRANSPARENT, Color32::TRANSPARENT, p.text2),
            IconButtonKind::Bordered => (p.surface, p.border, p.text),
            IconButtonKind::Danger => (Color32::TRANSPARENT, Color32::TRANSPARENT, p.danger),
        };
        if selected {
            fill = p.accent_subtle;
            stroke = Color32::TRANSPARENT;
            fg = p.accent_text;
        } else if pressed {
            fill = if kind == IconButtonKind::Danger {
                p.danger_subtle
            } else {
                p.pressed
            };
        } else if hovered {
            fill = if kind == IconButtonKind::Danger {
                p.danger_subtle
            } else {
                p.hover
            };
            if kind == IconButtonKind::Ghost {
                fg = p.text;
            }
            if kind == IconButtonKind::Bordered {
                stroke = p.border_strong;
            }
        }
        let painter = ui.painter();
        painter.rect(
            rect,
            CornerRadius::same(size::R_CONTROL),
            fade(fill, enabled),
            Stroke::new(1.0, fade(stroke, enabled)),
            StrokeKind::Inside,
        );
        painter.text(
            rect.center(),
            Align2::CENTER_CENTER,
            icon,
            font_icon(size::ICON),
            fade(fg, enabled),
        );
        if resp.has_focus() {
            focus_ring(ui, rect, size::R_CONTROL);
        }
    }
    resp.on_hover_text(tooltip)
}

/// Left-hand navigation entry ("tab"). Full width of the sidebar, 36 px tall.
pub(crate) fn nav_item(ui: &mut Ui, icon: &str, label: &str, selected: bool) -> Response {
    let p = Palette::of(ui.ctx());
    let (rect, resp) =
        ui.allocate_exact_size(vec2(ui.available_width(), size::NAV_ITEM_H), Sense::click());
    resp.widget_info(|| WidgetInfo::selected(WidgetType::SelectableLabel, true, selected, label));
    if ui.is_rect_visible(rect) {
        let hovered = resp.hovered();
        let (fill, fg) = if selected {
            (p.accent_subtle, p.accent_text)
        } else if hovered {
            (p.hover, p.text)
        } else {
            (Color32::TRANSPARENT, p.text2)
        };
        let painter = ui.painter();
        painter.rect_filled(rect, CornerRadius::same(size::R_CARD), fill);
        let y = rect.center().y;
        painter.text(
            egui::pos2(rect.left() + 10.0, y),
            Align2::LEFT_CENTER,
            icon,
            font_icon(size::ICON),
            fg,
        );
        let font = if selected {
            font_medium(14.0)
        } else {
            font_regular(14.0)
        };
        painter.text(
            egui::pos2(rect.left() + 38.0, y),
            Align2::LEFT_CENTER,
            label,
            font,
            fg,
        );
        if resp.has_focus() {
            focus_ring(ui, rect, size::R_CARD);
        }
    }
    resp.on_hover_cursor(CursorIcon::PointingHand)
}

/// iOS-style switch for booleans (`allow_drive`, start with OS …).
pub(crate) fn toggle(ui: &mut Ui, on: &mut bool) -> Response {
    let p = Palette::of(ui.ctx());
    // Allocate the full control height so rows align, draw the 36×20 track centered.
    let (rect, mut resp) = ui.allocate_exact_size(vec2(36.0, size::CONTROL_H), Sense::click());
    if resp.clicked() {
        *on = !*on;
        resp.mark_changed();
    }
    resp.widget_info(|| WidgetInfo::selected(WidgetType::Checkbox, ui.is_enabled(), *on, ""));
    if ui.is_rect_visible(rect) {
        let t = ui.ctx().animate_bool_responsive(resp.id, *on);
        let track = egui::Rect::from_center_size(rect.center(), vec2(36.0, 20.0));
        let fill = if *on {
            if resp.hovered() {
                p.accent_hover
            } else {
                p.accent
            }
        } else if resp.hovered() {
            p.text3
        } else {
            p.border_strong
        };
        let painter = ui.painter();
        painter.rect_filled(track, CornerRadius::same(10), fade(fill, ui.is_enabled()));
        let x = egui::lerp((track.left() + 10.0)..=(track.right() - 10.0), t);
        painter.circle_filled(egui::pos2(x, track.center().y), 8.0, Color32::WHITE);
        if resp.has_focus() {
            focus_ring(ui, track, 10);
        }
    }
    resp
}

/// Segmented control for small enums, e.g. clipboard both/send/receive/ask/off.
pub(crate) fn segmented<T: PartialEq + Copy>(
    ui: &mut Ui,
    value: &mut T,
    options: &[(T, &str)],
) -> Response {
    let p = Palette::of(ui.ctx());
    let width: f32 = options
        .iter()
        .map(|(_, label)| {
            ui.painter()
                .layout_no_wrap((*label).to_owned(), font_medium(13.0), p.text)
                .size()
                .x
                + 26.0
        })
        .sum();
    let inner = Frame::new()
        .fill(p.hover)
        .stroke(Stroke::new(1.0, p.border))
        .corner_radius(CornerRadius::same(size::R_CARD))
        .inner_margin(Margin::same(2))
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            ui.allocate_ui_with_layout(
                vec2(width, size::CONTROL_H_SM),
                Layout::left_to_right(Align::Center),
                |ui| {
                    let mut changed = false;
                    for (opt, label) in options {
                        let selected = *value == *opt;
                        let font = font_medium(13.0);
                        let g = ui.painter().layout_no_wrap(
                            label.to_string(),
                            font,
                            Color32::PLACEHOLDER,
                        );
                        let (rect, resp) = ui.allocate_exact_size(
                            vec2(g.size().x + 24.0, size::CONTROL_H_SM),
                            Sense::click(),
                        );
                        resp.widget_info(|| {
                            WidgetInfo::selected(WidgetType::RadioButton, true, selected, *label)
                        });
                        if resp.clicked() && !selected {
                            *value = *opt;
                            changed = true;
                        }
                        let painter = ui.painter();
                        if selected {
                            painter.rect(
                                rect,
                                CornerRadius::same(size::R_CONTROL),
                                p.surface,
                                Stroke::new(1.0, p.border),
                                StrokeKind::Inside,
                            );
                        } else if resp.hovered() {
                            painter.rect_filled(
                                rect,
                                CornerRadius::same(size::R_CONTROL),
                                p.pressed,
                            );
                        }
                        let fg = if selected || resp.hovered() {
                            p.text
                        } else {
                            p.text2
                        };
                        painter.galley(rect.center() - g.size() / 2.0, g, fg);
                        if resp.has_focus() {
                            focus_ring(ui, rect, size::R_CONTROL);
                        }
                    }
                    changed
                },
            )
            .inner
        });
    let mut resp = inner.response;
    if inner.inner {
        resp.mark_changed();
    }
    resp
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum BadgeKind {
    Online,
    Offline,
    Accent,
    Warning,
    Danger,
}

/// Status pill. Always carries text (never color alone).
pub(crate) fn badge(ui: &mut Ui, kind: BadgeKind, text: &str) -> Response {
    let p = Palette::of(ui.ctx());
    let (bg, fg, dot) = match kind {
        BadgeKind::Online => (p.success_subtle, p.success, true),
        BadgeKind::Offline => (p.hover, p.text2, true),
        BadgeKind::Accent => (p.accent_subtle, p.accent_text, false),
        BadgeKind::Warning => (p.warning_subtle, p.warning, true),
        BadgeKind::Danger => (p.danger_subtle, p.danger, true),
    };
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_owned(), font_medium(12.0), fg);
    let dot_width = if dot { 12.0 } else { 0.0 };
    let (rect, response) = ui.allocate_exact_size(
        vec2(galley.size().x + 16.0 + dot_width, 22.0),
        Sense::hover(),
    );
    ui.painter().rect_filled(rect, size::R_PILL, bg);
    if dot {
        let center = egui::pos2(rect.left() + 10.0, rect.center().y);
        if kind == BadgeKind::Offline {
            ui.painter()
                .circle_stroke(center, 2.5, Stroke::new(1.5, p.text3));
        } else {
            ui.painter().circle_filled(center, 3.0, fg);
        }
    }
    ui.painter().galley(
        egui::pos2(
            rect.left() + 8.0 + dot_width,
            rect.center().y - galley.size().y / 2.0,
        ),
        galley,
        fg,
    );
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Label, ui.is_enabled(), text));
    response
}

/// Shared display names for config keys used in chords and modifier remaps.
pub(crate) fn key_name(key: &str) -> String {
    key.trim()
        .to_ascii_lowercase()
        .split('_')
        .map(|part| match part {
            "left" => "L".to_owned(),
            "right" => "R".to_owned(),
            "meta" | "super" | "win" => match std::env::consts::OS {
                "windows" => "Win",
                "macos" => "Cmd",
                _ => "Super",
            }
            .to_owned(),
            other => {
                let mut word = other.to_owned();
                if let Some(first) = word.get_mut(..1) {
                    first.make_ascii_uppercase();
                }
                word
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// A key cap ("Ctrl", "Alt", "L") for hotkeys and modifier remaps.
pub(crate) fn keycap(ui: &mut Ui, key: &str) -> Response {
    let p = Palette::of(ui.ctx());
    Frame::new()
        .fill(p.hover)
        .stroke(Stroke::new(1.0, p.border_strong))
        .corner_radius(CornerRadius::same(size::R_KEYCAP))
        .inner_margin(Margin::symmetric(7, 2))
        .show(ui, |ui| {
            ui.label(
                RichText::new(key_name(key))
                    .font(font_mono(12.0))
                    .color(p.text),
            );
        })
        .response
}

/// A hotkey chord shown as keycaps: "ctrl+alt+shift+l" → [Ctrl] [Alt] [Shift] [L].
pub(crate) fn chord(ui: &mut Ui, chord: &str) {
    let keys: Vec<_> = chord.split('+').collect();
    let width: f32 = keys
        .iter()
        .map(|key| {
            ui.painter()
                .layout_no_wrap(key_name(key), font_mono(12.0), Palette::of(ui.ctx()).text)
                .size()
                .x
                + 18.0
        })
        .sum();
    ui.allocate_ui_with_layout(
        vec2(width, size::CONTROL_H),
        Layout::left_to_right(Align::Center),
        |ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            for key in keys {
                keycap(ui, key);
            }
        },
    );
}

/// Bordered surface card, 20 px padding.
pub(crate) fn card<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    let p = Palette::of(ui.ctx());
    Frame::new()
        .fill(ui.visuals().widgets.noninteractive.bg_fill)
        .stroke(Stroke::new(1.0, p.border))
        .corner_radius(CornerRadius::same(size::R_CARD))
        .inner_margin(Margin::same(20))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui)
        })
        .inner
}

/// Uppercase mono section label ("PAIRED MACHINES").
pub(crate) fn overline(ui: &mut Ui, text: &str) -> Response {
    let p = Palette::of(ui.ctx());
    ui.label(
        RichText::new(text.to_uppercase())
            .text_style(overline_style())
            .color(p.text3)
            .extra_letter_spacing(0.9),
    )
}

/// Page title + one-line description on the left, actions on the right.
pub(crate) fn page_header(ui: &mut Ui, title: &str, subtitle: &str, actions: impl FnOnce(&mut Ui)) {
    let p = Palette::of(ui.ctx());
    let heading = |ui: &mut Ui| {
        ui.spacing_mut().item_spacing.y = 4.0;
        ui.label(RichText::new(title).text_style(title_style()).color(p.text));
        ui.label(RichText::new(subtitle).color(p.text2));
    };
    if ui.available_width() < 800.0 {
        ui.vertical(heading);
        ui.horizontal(|ui| {
            ui.with_layout(Layout::right_to_left(Align::Center), actions);
        });
    } else {
        let heading_width = ui.available_width() * 0.48;
        ui.horizontal(|ui| {
            ui.allocate_ui_with_layout(
                vec2(heading_width, 0.0),
                Layout::top_down(Align::Min),
                |ui| {
                    ui.set_width(heading_width);
                    heading(ui);
                },
            );
            ui.with_layout(Layout::right_to_left(Align::Center), actions);
        });
    }
    ui.add_space(size::SECTION_GAP - 8.0);
}

/// One setting inside a card: title + help (+ config key) on the left, control on the right.
pub(crate) fn setting_row(
    ui: &mut Ui,
    title: &str,
    help: &str,
    config_key: Option<&str>,
    control: impl FnOnce(&mut Ui),
) {
    let p = Palette::of(ui.ctx());
    let label = |ui: &mut Ui| {
        ui.spacing_mut().item_spacing.y = 2.0;
        ui.label(RichText::new(title).font(font_medium(14.0)).color(p.text));
        ui.label(RichText::new(help).small().color(p.text3));
        if let Some(key) = config_key {
            ui.label(RichText::new(key).font(font_mono(12.0)).color(p.text3));
        }
    };
    let label_width = ui.available_width() * 0.48;
    ui.horizontal_top(|ui| {
        ui.allocate_ui_with_layout(vec2(label_width, 0.0), Layout::top_down(Align::Min), |ui| {
            ui.set_width(label_width);
            label(ui);
        });
        ui.allocate_ui_with_layout(
            vec2(ui.available_width(), size::CONTROL_H),
            Layout::top_down(Align::Max),
            control,
        );
    });
}

/// Labelled single-line input. `mono` for addresses, codes, IDs.
pub(crate) fn text_input(
    ui: &mut Ui,
    label: &str,
    value: &mut String,
    hint: &str,
    mono: bool,
) -> Response {
    let p = Palette::of(ui.ctx());
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 6.0;
        ui.label(RichText::new(label).font(font_medium(13.0)).color(p.text));
        let font = if mono {
            font_mono(13.0)
        } else {
            font_regular(14.0)
        };
        ui.add(
            TextEdit::singleline(value)
                .hint_text(WidgetText::from(RichText::new(hint).color(p.text3)))
                .font(font)
                .margin(Margin::symmetric(10, 7))
                .desired_width(f32::INFINITY),
        )
    })
    .inner
}

/// Re-export the icons you use, so call sites read `theme::icon::MONITOR`.
pub(crate) mod icon {
    pub(crate) use super::ph::{
        ARROW_CLOCKWISE, ARROW_RIGHT, BROADCAST, COPY, FLOPPY_DISK, KEYBOARD, MONITOR, PAUSE, PLAY,
        POWER, SHIELD_CHECK, SQUARE_SPLIT_HORIZONTAL, WARNING_CIRCLE, X_CIRCLE,
    };
}

/// Quiet help text shared by screens.
pub(crate) fn small(ui: &mut Ui, text: &str) {
    ui.label(
        RichText::new(text)
            .small()
            .color(Palette::of(ui.ctx()).text3),
    );
}

pub(crate) fn error(ui: &mut Ui, text: &str) {
    let p = Palette::of(ui.ctx());
    ui.horizontal_wrapped(|ui| {
        ui.label(
            RichText::new(icon::WARNING_CIRCLE)
                .font(font_icon(size::ICON))
                .color(p.danger),
        );
        ui.label(RichText::new(text).small().color(p.danger));
    });
}

pub(crate) fn icon_tile(ui: &mut Ui, icon: &str) {
    let p = Palette::of(ui.ctx());
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(40.0), Sense::hover());
    ui.painter().rect_filled(rect, size::R_CARD, p.hover);
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        icon,
        font_icon(size::ICON_LG),
        p.text2,
    );
}

/// Empty sections retain a visible boundary and an explanation.
pub(crate) fn empty_card(ui: &mut Ui, icon: &str, sentence: &str, hint: &str) {
    let p = Palette::of(ui.ctx());
    let response = Frame::new()
        .inner_margin(20)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                icon_tile(ui, icon);
                ui.vertical(|ui| {
                    ui.label(sentence);
                    small(ui, hint);
                });
            });
        })
        .response;
    let rect = response.rect;
    let points = [
        rect.left_top(),
        rect.right_top(),
        rect.right_bottom(),
        rect.left_bottom(),
        rect.left_top(),
    ];
    ui.painter().extend(egui::Shape::dashed_line(
        &points,
        Stroke::new(1.0, p.border_strong),
        4.0,
        3.0,
    ));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_name_config_tokens_use_consistent_human_labels() {
        // Arrange
        let meta = match std::env::consts::OS {
            "windows" => "Win",
            "macos" => "Cmd",
            _ => "Super",
        };
        let cases = [
            ("right_alt", "R Alt".to_owned()),
            ("scroll_lock", "Scroll Lock".to_owned()),
            ("home", "Home".to_owned()),
            ("ctrl", "Ctrl".to_owned()),
            ("l", "L".to_owned()),
            ("left_ctrl", "L Ctrl".to_owned()),
            ("page_up", "Page Up".to_owned()),
            ("f12", "F12".to_owned()),
            ("meta", meta.to_owned()),
            ("left_meta", format!("L {meta}")),
            ("right_meta", format!("R {meta}")),
            ("super", meta.to_owned()),
            ("win", meta.to_owned()),
        ];

        for (key, expected) in cases {
            // Act
            let actual = key_name(key);

            // Assert
            assert_eq!(actual, expected, "{key}");
        }
    }
}
