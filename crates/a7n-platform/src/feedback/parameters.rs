// SPDX-License-Identifier: GPL-3.0-or-later

//! Shared bounds and HTML defaults for config validation and the design sliders.

#[derive(Debug, Clone, Copy)]
pub struct Parameter {
    pub key: &'static str,
    pub min: f32,
    pub max: f32,
    pub default: f32,
    pub step: f64,
}

pub const DEPTH: Parameter = Parameter {
    key: "feedback.depth",
    min: 20.0,
    max: 160.0,
    default: 60.0,
    step: 1.0,
};
pub const RADIUS: Parameter = Parameter {
    key: "feedback.radius",
    min: 60.0,
    max: 360.0,
    default: 160.0,
    step: 1.0,
};
pub const SPEED: Parameter = Parameter {
    key: "feedback.speed",
    min: 0.2,
    max: 3.5,
    default: 1.3,
    step: 0.1,
};
pub const FREQUENCY: Parameter = Parameter {
    key: "feedback.frequency",
    min: 0.4,
    max: 2.5,
    default: 1.0,
    step: 0.1,
};
pub const INTENSITY: Parameter = Parameter {
    key: "feedback.intensity",
    min: 0.4,
    max: 2.2,
    default: 1.2,
    step: 0.1,
};
pub const ALL: [Parameter; 5] = [DEPTH, RADIUS, SPEED, FREQUENCY, INTENSITY];
