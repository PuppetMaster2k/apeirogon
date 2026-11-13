// SPDX-License-Identifier: GPL-3.0-or-later

//! The fixed feedback palette, shared by the renderer and GUI swatches.

use super::Color;

struct Entry {
    color: Color,
    key: &'static str,
    name: &'static str,
    stops: &'static [[u8; 3]],
}

const PALETTE: &[Entry] = &[
    Entry {
        color: Color::Accent,
        key: "accent",
        name: "Accent",
        stops: &[[0x2f, 0x5b, 0xea]],
    },
    Entry {
        color: Color::Green,
        key: "green",
        name: "Green",
        stops: &[[0x43, 0xc7, 0x7f]],
    },
    Entry {
        color: Color::Orange,
        key: "orange",
        name: "Orange",
        stops: &[[0xe9, 0xa2, 0x3b]],
    },
    Entry {
        color: Color::Pink,
        key: "pink",
        name: "Pink",
        stops: &[[0xed, 0x78, 0xb5]],
    },
    Entry {
        color: Color::Ibtesam,
        key: "ibtesam",
        name: "Ibtesam",
        stops: &[[0x00, 0xf5, 0xa0], [0x00, 0xd9, 0xf5]],
    },
    Entry {
        color: Color::Lunada,
        key: "lunada",
        name: "Lunada",
        stops: &[[0x54, 0x33, 0xff], [0x20, 0xbd, 0xff], [0xa5, 0xfe, 0xcb]],
    },
    Entry {
        color: Color::Stripe,
        key: "stripe",
        name: "Stripe",
        stops: &[[0x1f, 0xa2, 0xff], [0x12, 0xd8, 0xfa], [0xa6, 0xff, 0xcb]],
    },
    Entry {
        color: Color::Ali,
        key: "ali",
        name: "Ali",
        stops: &[[0xff, 0x4b, 0x1f], [0x1f, 0xdd, 0xff]],
    },
    Entry {
        color: Color::Alihossein,
        key: "alihossein",
        name: "Alihossein",
        stops: &[[0xf7, 0xff, 0x00], [0xdb, 0x36, 0xa4]],
    },
    Entry {
        color: Color::Transfile,
        key: "transfile",
        name: "Transfile",
        stops: &[[0x16, 0xbf, 0xfd], [0xcb, 0x30, 0x66]],
    },
    Entry {
        color: Color::ElectricViolet,
        key: "electric_violet",
        name: "Electric Violet",
        stops: &[[0x47, 0x76, 0xe6], [0x8e, 0x54, 0xe9]],
    },
    Entry {
        color: Color::Martini,
        key: "martini",
        name: "Martini",
        stops: &[[0xfd, 0xfc, 0x47], [0x24, 0xfe, 0x41]],
    },
    Entry {
        color: Color::Superman,
        key: "superman",
        name: "Superman",
        stops: &[[0x00, 0x99, 0xf7], [0xf1, 0x17, 0x12]],
    },
    Entry {
        color: Color::EdsSunsetGradient,
        key: "eds_sunset_gradient",
        name: "Ed's Sunset Gradient",
        stops: &[[0xff, 0x7e, 0x5f], [0xfe, 0xb4, 0x7b]],
    },
    Entry {
        color: Color::SublimeLight,
        key: "sublime_light",
        name: "Sublime Light",
        stops: &[[0xfc, 0x5c, 0x7d], [0x6a, 0x82, 0xfb]],
    },
    Entry {
        color: Color::Argon,
        key: "argon",
        name: "Argon",
        stops: &[
            [0x03, 0x00, 0x1e],
            [0x73, 0x03, 0xc0],
            [0xec, 0x38, 0xbc],
            [0xfd, 0xef, 0xf9],
        ],
    },
    Entry {
        color: Color::JShine,
        key: "j_shine",
        name: "JShine",
        stops: &[[0x12, 0xc2, 0xe9], [0xc4, 0x71, 0xed], [0xf6, 0x4f, 0x59]],
    },
    Entry {
        color: Color::Flare,
        key: "flare",
        name: "Flare",
        stops: &[[0xff, 0xf3, 0xb0], [0xf5, 0xaf, 0x19], [0xf1, 0x27, 0x11]],
    },
    Entry {
        color: Color::UltraVoilet,
        key: "ultra_voilet",
        name: "Ultra Voilet",
        stops: &[[0x65, 0x4e, 0xa3], [0xea, 0xaf, 0xc8]],
    },
    Entry {
        color: Color::KyooPal,
        key: "kyoo_pal",
        name: "Kyoo Pal",
        stops: &[[0xdd, 0x3e, 0x54], [0x6b, 0xe5, 0x85]],
    },
    Entry {
        color: Color::KyeMeh,
        key: "kye_meh",
        name: "Kye Meh",
        stops: &[[0x83, 0x60, 0xc3], [0x2e, 0xbf, 0x91]],
    },
    Entry {
        color: Color::Wiretap,
        key: "wiretap",
        name: "Wiretap",
        stops: &[[0x8a, 0x23, 0x87], [0xe9, 0x40, 0x57], [0xf2, 0x71, 0x21]],
    },
    Entry {
        color: Color::Rastafari,
        key: "rastafari",
        name: "Rastafari",
        stops: &[[0x1e, 0x96, 0x00], [0xff, 0xf2, 0x00], [0xff, 0x00, 0x00]],
    },
    Entry {
        color: Color::SinCityRed,
        key: "sin_city_red",
        name: "Sin City Red",
        stops: &[[0xed, 0x21, 0x3a], [0x93, 0x29, 0x1e]],
    },
    Entry {
        color: Color::AzurLane,
        key: "azur_lane",
        name: "Azur Lane",
        stops: &[[0x7f, 0x7f, 0xd5], [0x86, 0xa8, 0xe7], [0x91, 0xea, 0xe4]],
    },
    Entry {
        color: Color::WitchingHour,
        key: "witching_hour",
        name: "Witching Hour",
        stops: &[[0xc3, 0x14, 0x32], [0x24, 0x0b, 0x36]],
    },
];

impl Color {
    /// All supported colours in presentation order, with stable config values and names.
    pub fn choices() -> impl Iterator<Item = (Self, &'static str, &'static str)> {
        PALETTE
            .iter()
            .map(|entry| (entry.color, entry.key, entry.name))
    }

    /// Samples evenly spaced colour stops. Values outside the edge clamp to its ends.
    #[must_use]
    // Stop indexes and interpolated channels are bounded before conversion.
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss
    )]
    pub fn sample(self, t: f32, dark: bool) -> [u8; 3] {
        if self == Self::Accent && dark {
            return [0x70, 0x88, 0xff];
        }
        let stops = PALETTE[self as usize].stops;
        let position = if t.is_nan() { 0.0 } else { t.clamp(0.0, 1.0) } * (stops.len() - 1) as f32;
        let index = (position as usize).min(stops.len() - 1);
        let next = (index + 1).min(stops.len() - 1);
        let fraction = position - index as f32;
        std::array::from_fn(|channel| {
            let start = f32::from(stops[index][channel]);
            (start + (f32::from(stops[next][channel]) - start) * fraction).round() as u8
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sample_multi_stop_gradient_returns_endpoints_and_interpolation() {
        // Arrange
        let color = Color::Lunada;

        // Act
        let samples = [0.0, 0.25, 0.5, 0.75, 1.0].map(|t| color.sample(t, false));

        // Assert
        assert_eq!(
            samples,
            [
                [0x54, 0x33, 0xff],
                [0x3a, 0x78, 0xff],
                [0x20, 0xbd, 0xff],
                [0x63, 0xde, 0xe5],
                [0xa5, 0xfe, 0xcb]
            ]
        );
    }
}
