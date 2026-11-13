// SPDX-License-Identifier: GPL-3.0-or-later

use std::f32::consts::FRAC_PI_2;

use tiny_skia::PathBuilder;

use super::{Canvas, Push};

const LAYERS: u16 = 4;
const STEPS: u16 = 70;
pub(super) fn render(canvas: &mut Canvas<'_>, push: &Push) {
    if push.proximity < 0.01 {
        return;
    }
    let (start, end, half) = push.span(1.15);
    for l in 0..LAYERS {
        let layer = f32::from(l) / f32::from(LAYERS - 1);
        let base = push.look.depth * (0.18 + layer * 0.82) * push.proximity;
        let amp = push.look.depth * 0.28 * (1.0 - layer * 0.35) * push.proximity;
        let alpha = ((0.65 - layer * 0.42) * push.gain()).min(1.0);
        let mut path = PathBuilder::new();
        path.move_to(start, 0.0);
        for i in 0..=STEPS {
            let pos = start + (end - start) * f32::from(i) / f32::from(STEPS);
            let envelope = (((pos - push.pointer).abs() / half * FRAC_PI_2)
                .cos()
                .max(0.0))
            .powf(1.6);
            let wave = ((pos + push.start) * 0.045 * push.look.frequency
                + push.phase * (1.4 + f32::from(l) * 0.3)
                + f32::from(l) * 1.3)
                .sin()
                * 0.65
                + ((pos + push.start) * 0.09 * push.look.frequency
                    - push.phase * 1.1
                    - f32::from(l))
                .cos()
                    * 0.35;
            path.line_to(pos, ((base + wave * amp) * envelope).max(0.0));
        }
        path.line_to(end, 0.0);
        path.close();
        if let Some(path) = path.finish() {
            canvas.solid(layer, alpha * 0.38);
            canvas.fill(&path);
            canvas.solid(layer, alpha * 0.9);
            canvas.stroke(&path, 1.8 - layer * 0.8);
        }
    }
}
