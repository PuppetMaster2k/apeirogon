// SPDX-License-Identifier: GPL-3.0-or-later

use tiny_skia::PathBuilder;

use super::{Canvas, Push, Random};

// Maximum HTML spawn rate (450/s) times maximum lifetime (1.2s), with headroom.
const MAX_PARTICLES: usize = 640;
// Preserve the reference seed range rather than substituting a full turn.
#[allow(clippy::approx_constant)]
const SEED_RANGE: f32 = 6.28;
struct Particle {
    x: f32,
    y: f32,
    vx: f32,
    vy: f32,
    age: f32,
    life: f32,
    size: f32,
    seed: f32,
}
#[derive(Default)]
pub(super) struct State {
    particles: Vec<Particle>,
    acc: f32,
}
impl State {
    pub fn reset(&mut self) {
        self.particles.clear();
        self.acc = 0.0;
    }
}
pub(super) fn render(
    canvas: &mut Canvas<'_>,
    push: &Push,
    state: &mut State,
    random: &mut Random,
    dt: f32,
) {
    if push.proximity > 0.01 {
        canvas.spot(
            push,
            0.65,
            &[(0.0, 0.15, 0.55), (0.5, 0.6, 0.2), (1.0, 0.0, 0.0)],
        );
    }
    if !push.animate {
        return;
    }
    if state.particles.capacity() < MAX_PARTICLES {
        state
            .particles
            .reserve(MAX_PARTICLES - state.particles.len());
    }
    if push.proximity > 0.01 {
        state.acc += 180.0 * push.look.frequency * push.proximity.powi(2) * dt;
        while state.acc >= 1.0 {
            state.acc -= 1.0;
            if state.particles.len() >= MAX_PARTICLES {
                continue;
            }
            let spread = (random.unit() - 0.5) * push.look.radius * 0.85;
            let inward = (45.0 + random.unit() * 110.0) * push.look.depth / 55.0;
            let tangent = (random.unit() - 0.5) * 80.0;
            state.particles.push(Particle {
                x: (push.pointer + spread).clamp(
                    2.0 - push.start,
                    (push.length - push.start - 2.0).max(2.0 - push.start),
                ),
                y: 2.0,
                vx: tangent,
                vy: inward,
                age: 0.0,
                life: 0.45 + random.unit() * 0.75,
                size: 1.3 + random.unit() * 2.3,
                seed: random.unit() * SEED_RANGE,
            });
        }
    }
    state.particles.retain_mut(|s| {
        s.age += dt;
        if s.age >= s.life {
            return false;
        }
        let (px, py) = (s.x, s.y);
        // Swirl and gravity stay in desktop axes when the pushed edge rotates.
        let swirl = (push.phase * 9.0 + s.seed + s.age * 6.0).sin() * 55.0 * dt * 0.4;
        let (ax, ay) = match push.edge {
            super::Edge::Top => (swirl, -16.0 * dt),
            super::Edge::Bottom => (swirl, 16.0 * dt),
            super::Edge::Right => (-16.0 * dt, -swirl),
            _ => (-16.0 * dt, swirl),
        };
        s.vx = s.vx * 0.96 + ax;
        s.vy = s.vy * 0.96 + ay;
        s.x += s.vx * dt;
        s.y += s.vy * dt;
        let life = s.age / s.life;
        let flicker = 0.75 + 0.25 * (push.phase * 35.0 + s.seed).sin();
        let alpha = ((1.0 - life).powf(1.3) * flicker * push.look.intensity).min(1.0);
        let mut path = PathBuilder::new();
        path.move_to(px, py);
        path.line_to(s.x, s.y);
        canvas.solid(life, alpha * 0.85);
        if let Some(path) = path.finish() {
            canvas.stroke(&path, s.size * (1.0 - life * 0.5));
        }
        canvas.solid(life, alpha);
        canvas.circle(s.x, s.y, s.size * 0.65 * (1.0 - life * 0.4));
        true
    });
}
