// SPDX-License-Identifier: GPL-3.0-or-later

//! Desktop-free PNG contact sheets using the native feedback renderer.

use std::fs::{self, File};
use std::io::{self, BufWriter};
use std::path::Path;
use std::time::Duration;

use a7n_proto::v1::Edge;
use tiny_skia::{Pixmap, PixmapPaint, PixmapRef, Transform};

use super::look::{self, Rect};
use super::raster::{self, Along, Arrival, EdgeEffect, Frame};
use super::timeline::{End, Timeline, arrival_duration, departure_duration};
use super::{Color, Landing, Look, Size, Style};

const WIDTH: u32 = 1600;
const HEIGHT: u32 = 1000;
const PUSH_Y: u32 = HEIGHT * 2 / 5;
const CROP: u32 = 480;
const GAP: u32 = 4;
const FRAMES: u32 = 8;
const DARK: [u8; 3] = [0x0e, 0x11, 0x16];
const LIGHT: [u8; 3] = [0xf4, 0xf5, 0xf7];
const PUSH_PROGRESS: [u16; 6] = [250, 500, 750, 1000, 1000, 1000];
const PUSH_STEP: Duration = Duration::from_millis(200);

/// Writes departure, arrival and full-edge PNGs into `out_dir`.
///
/// # Errors
/// Returns an error if allocating a pixmap, creating the directory or writing a PNG fails.
pub fn sheet(
    style: Style,
    color: Color,
    size: Size,
    landing: Landing,
    out_dir: &Path,
) -> io::Result<()> {
    fs::create_dir_all(out_dir)?;
    let look = Look {
        style,
        landing,
        color,
        size,
        ..Look::default()
    };
    let name = format!("{style:?}").to_ascii_lowercase();
    for arrival in [false, true] {
        let mut sheet = pixmap(FRAMES * CROP + (FRAMES - 1) * GAP, CROP * 2 + GAP)?;
        for (row, background) in [DARK, LIGHT].into_iter().enumerate() {
            for index in 0..FRAMES {
                let frame = snapshot(look, row == 0, index, arrival)?;
                let mut crop = pixmap(CROP, CROP)?;
                crop.fill(tiny_skia::Color::from_rgba8(
                    background[0],
                    background[1],
                    background[2],
                    255,
                ));
                let x = if arrival { WIDTH - CROP } else { 0 };
                blit(
                    &mut crop,
                    frame.as_ref(),
                    -i32::try_from(x).map_err(io::Error::other)?,
                    -i32::try_from(PUSH_Y - CROP / 2).map_err(io::Error::other)?,
                );
                blit(
                    &mut sheet,
                    crop.as_ref(),
                    i32::try_from(index * (CROP + GAP)).map_err(io::Error::other)?,
                    i32::try_from(row).map_err(io::Error::other)?
                        * i32::try_from(CROP + GAP).map_err(io::Error::other)?,
                );
            }
        }
        let kind = if arrival { "arrival" } else { "departure" };
        save(&sheet, &out_dir.join(format!("{name}-{kind}.png")))?;
    }
    // A horizontal edge shows the requested full 1600-pixel span at 1:1.
    let (mut frame, rect, effect) = departure(look, true, 750, Edge::Top);
    let mut timeline = Timeline::default();
    timeline.push(effect, Duration::ZERO);
    for step in if matches!(style, Style::Sparks | Style::Arcs) {
        0..=60_u32
    } else {
        60..=60
    } {
        timeline.render_edge(&mut frame, Duration::from_secs_f64(f64::from(step) / 60.0));
    }
    let mut strip = pixmap(WIDTH, frame.height)?;
    strip.fill(tiny_skia::Color::from_rgba8(DARK[0], DARK[1], DARK[2], 255));
    overlay(&mut strip, &frame, rect)?;
    save(&strip, &out_dir.join(format!("{name}-strip.png")))
}

fn pixmap(width: u32, height: u32) -> io::Result<Pixmap> {
    Pixmap::new(width, height).ok_or_else(|| io::Error::other("could not allocate preview pixmap"))
}

fn blit(target: &mut Pixmap, source: PixmapRef<'_>, x: i32, y: i32) {
    target.draw_pixmap(
        x,
        y,
        source,
        &PixmapPaint::default(),
        Transform::identity(),
        None,
    );
}

// All preview geometry is bounded by the fixed simulated monitor.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn overlay(target: &mut Pixmap, frame: &Frame, rect: Rect) -> io::Result<()> {
    let source = PixmapRef::from_bytes(&frame.pixels, frame.width, frame.height)
        .ok_or_else(|| io::Error::other("invalid preview frame"))?;
    blit(target, source, rect.x.round() as i32, rect.y.round() as i32);
    Ok(())
}

const fn area() -> Rect {
    Rect {
        x: 0.0,
        y: 0.0,
        width: WIDTH as f64,
        height: HEIGHT as f64,
    }
}

// Preview dimensions are small, fixed and rounded like native pixel geometry.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn frame(rect: Rect, dark: bool) -> Frame {
    let mut frame = Frame {
        animate: true,
        dark,
        opacity: 1.0,
        ..Frame::default()
    };
    frame.resize(
        rect.width.round().max(1.0) as u32,
        rect.height.round().max(1.0) as u32,
    );
    frame
}

fn departure(look: Look, dark: bool, progress: u16, edge: Edge) -> (Frame, Rect, EdgeEffect) {
    let horizontal = edge == Edge::Top;
    let span = if horizontal { WIDTH } else { HEIGHT };
    let push = if horizontal {
        f64::from(WIDTH) * 0.4
    } else {
        f64::from(PUSH_Y)
    };
    let depth = f64::from(raster::OVERLAY_DEPTH);
    let rect = if horizontal {
        Rect {
            x: 0.0,
            y: 0.0,
            width: f64::from(WIDTH),
            height: depth.min(f64::from(HEIGHT)),
        }
    } else {
        Rect {
            x: 0.0,
            y: 0.0,
            width: depth.min(f64::from(WIDTH)),
            height: f64::from(HEIGHT),
        }
    };
    let start = if horizontal { rect.x } else { rect.y };
    let effect = EdgeEffect {
        look,
        edge,
        progress,
        along: Along {
            origin: (0.0, 0.0),
            start,
            span: f64::from(span),
            push: push - start,
        },
    };
    (frame(rect, dark), rect, effect)
}

fn snapshot(look: Look, dark: bool, index: u32, arrival: bool) -> io::Result<Pixmap> {
    let mut timeline = Timeline::default();
    let (frame, rect) = if arrival {
        let point = (f64::from(WIDTH - CROP / 2), f64::from(PUSH_Y));
        let diameter = look.arrival_diameter();
        let rect = look::landing_rect(area(), point, diameter);
        let mut frame = frame(rect, dark);
        frame.arrival = Some(Arrival::at(rect, point, 1.0));
        timeline.land(look, Duration::ZERO);
        let now = arrival_duration(look.style, true) * index / (FRAMES - 1);
        let steps = if matches!(look.style, Style::Sparks | Style::Arcs) {
            0..=120_u32
        } else {
            120..=120
        };
        for step in steps {
            let at = Duration::from_secs_f64(f64::from(step) / 60.0).min(now);
            timeline.render_landing(&mut frame, at);
            if at == now {
                break;
            }
        }
        (frame, rect)
    } else {
        departure_frame(&mut timeline, look, dark, index)
    };
    let mut result = pixmap(WIDTH, HEIGHT)?;
    overlay(&mut result, &frame, rect)?;
    Ok(result)
}

fn departure_frame(timeline: &mut Timeline, look: Look, dark: bool, index: u32) -> (Frame, Rect) {
    let progress = PUSH_PROGRESS.get(index as usize).copied().unwrap_or(1000);
    let (mut frame, rect, mut effect) = departure(look, dark, progress, Edge::Left);
    let crossed = PUSH_STEP * 6;
    let now = if index < 6 {
        PUSH_STEP * (index + 1)
    } else {
        crossed + departure_duration(look.style, true) * (index - 6) / 2
    };
    if !matches!(look.style, Style::Sparks | Style::Arcs) {
        timeline.push(effect, Duration::ZERO);
        if index >= 6 {
            timeline.end(End::Crossed, crossed);
        }
        timeline.render_edge(&mut frame, now);
        return (frame, rect);
    }
    for step in 0..=100_u32 {
        let at = Duration::from_secs_f64(f64::from(step) / 60.0).min(now);
        if at < crossed {
            effect.progress = PUSH_PROGRESS[(step / 12).min(5) as usize];
            timeline.push(effect, at);
        } else {
            timeline.end(End::Crossed, crossed);
        }
        timeline.render_edge(&mut frame, at);
        if at == now {
            break;
        }
    }
    (frame, rect)
}

fn save(pixmap: &Pixmap, path: &Path) -> io::Result<()> {
    let mut encoder = png::Encoder::new(
        BufWriter::new(File::create(path)?),
        pixmap.width(),
        pixmap.height(),
    );
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().map_err(io::Error::other)?;
    writer
        .write_image_data(pixmap.data())
        .map_err(io::Error::other)?;
    writer.finish().map_err(io::Error::other)
}
