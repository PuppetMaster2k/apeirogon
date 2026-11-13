// SPDX-License-Identifier: GPL-3.0-or-later

//! Plasma equations from Edge FX Studio, in its low-resolution field coordinates.
pub(super) fn sample(
    x: f32,
    y: f32,
    dx: f32,
    dy: f32,
    phase: f32,
    freq: f32,
    rim: f32,
) -> (f32, f32) {
    let nx = x * 0.045 * freq;
    let ny = y * 0.045 * freq;
    let w1 = (nx * 2.4 + phase + dx * 0.03).sin();
    let w2 = (ny * 2.4 - phase * 0.9 + dy * 0.03).cos();
    let w3 = ((nx + ny) * 1.8 - phase * 1.4).sin();
    let wave = (w1 + w2 + w3) / 6.0 + 0.5;
    (wave, (wave * 0.85 + rim.powi(4) * 0.55) * 1.4)
}
