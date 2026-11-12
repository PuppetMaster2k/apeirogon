// SPDX-License-Identifier: GPL-3.0-or-later

mod accordion;
mod badge;
mod button;
mod card;
mod disclosure;
pub(crate) mod icon;
mod icon_button;
mod keycap;
mod nav_item;
mod page_header;
mod paint;
mod palette;
mod segmented;
mod setting_row;
mod slider;
mod stepped_slider;
mod style;
mod text;
mod text_input;
mod toggle;
mod typography;

pub(crate) use badge::{BadgeKind, badge};
pub(crate) use button::{ButtonKind, ButtonSize, button};
pub(crate) use card::{card, empty_card, icon_tile};
pub(crate) use icon_button::{IconButtonKind, icon_button};
pub(crate) use keycap::{chord, key_name, keycap};
pub(crate) use nav_item::nav_item;
pub(crate) use page_header::{STACK_WIDTH, overline, page_header};
pub(crate) use palette::{DARK, LIGHT, Palette};
pub(crate) use segmented::segmented;
pub(crate) use setting_row::setting_row;
pub(crate) use style::install;
pub(crate) use text::{error, small, warning};
pub(crate) use text_input::{masked_text_input, text_input};
pub(crate) use toggle::toggle;
pub(crate) use typography::{
    family, font_icon, font_medium, font_mono, font_regular, font_semibold, overline_style, size,
    title_style,
};

pub(crate) use accordion::{accordion, accordion_group};
pub(crate) use disclosure::disclosure;
pub(crate) use slider::slider;
pub(crate) use stepped_slider::stepped_slider;
