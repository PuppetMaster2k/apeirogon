// SPDX-License-Identifier: GPL-3.0-or-later

//! Seeded value noise and the reference's three-octave fBm.
use std::sync::OnceLock;

use super::Random;

fn permutation() -> &'static [u8; 512] {
    static PERM: OnceLock<[u8; 512]> = OnceLock::new();
    PERM.get_or_init(|| {
        let mut perm = std::array::from_fn::<_, 256, _>(|i| i as u8);
        let mut random = Random::default();
        for i in (1..256).rev() {
            perm.swap(i, (random.unit() * (i + 1) as f32) as usize);
        }
        std::array::from_fn(|i| perm[i & 255])
    })
}
fn noise(x: f32, y: f32) -> f32 {
    let perm = permutation();
    let xi = (x.floor() as i32 & 255) as usize;
    let yi = (y.floor() as i32 & 255) as usize;
    let xf = x - x.floor();
    let yf = y - y.floor();
    let u = xf * xf * (3.0 - 2.0 * xf);
    let v = yf * yf * (3.0 - 2.0 * yf);
    let aa = f32::from(perm[usize::from(perm[xi]) + yi]) / 255.0;
    let ab = f32::from(perm[usize::from(perm[xi]) + yi + 1]) / 255.0;
    let ba = f32::from(perm[usize::from(perm[xi + 1]) + yi]) / 255.0;
    let bb = f32::from(perm[usize::from(perm[xi + 1]) + yi + 1]) / 255.0;
    let x1 = aa + u * (ba - aa);
    let x2 = ab + u * (bb - ab);
    x1 + v * (x2 - x1)
}
fn fbm(x: f32, y: f32) -> f32 {
    noise(x, y) * 0.55
        + noise(x * 2.03 + 11.4, y * 2.03 - 7.2) * 0.30
        + noise(x * 4.07 - 5.1, y * 4.07 + 19.8) * 0.15
}
pub(super) fn sample(x: f32, y: f32, phase: f32, freq: f32) -> (f32, f32) {
    let cx = x * 0.024 * freq + phase * 0.35;
    let cy = y * 0.024 * freq - phase * 0.18;
    let n1 = fbm(cx, cy);
    let n2 = fbm(cx * 1.7 - phase * 0.25, cy * 1.7 + 4.3);
    (
        (n1 * 1.15).min(1.0),
        ((n1 * 0.65 + n2 * 0.35) - 0.22).max(0.0) * 1.45 * 1.35,
    )
}
