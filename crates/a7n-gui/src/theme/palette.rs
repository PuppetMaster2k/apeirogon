// SPDX-License-Identifier: GPL-3.0-or-later

use eframe::egui::{Color32, Context, Theme};

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
