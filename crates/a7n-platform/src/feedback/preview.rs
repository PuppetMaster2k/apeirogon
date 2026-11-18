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
use super::timeline::{DEPARTURE_DURATION, End, Timeline, arrival_duration};
use super::{Color, Landing, Look, Size, Style};

const WIDTH: u32 = 1600;
const HEIGHT: u32 = 1000;
const PUSH_Y: u32 = HEIGHT * 2 / 5;
const CROP: u32 = 480;
const GAP: u32 = 4;
const FRAMES: u32 = 8;
#[allow(clippy::cast_possible_wrap, reason = "fixed preview width fits in i32")]
const ARRIVAL_CROP_X: i32 = (WIDTH - CROP) as i32;
#[allow(
    clippy::cast_possible_wrap,
    reason = "fixed preview height fits in i32"
)]
const CROP_Y: i32 = (PUSH_Y - CROP / 2) as i32;
#[allow(
    clippy::cast_possible_wrap,
    reason = "fixed preview cell size fits in i32"
)]
const CELL_STEP: i32 = (CROP + GAP) as i32;
const PREVIEW_FPS: u32 = 60;
const CROSSED_STEP: u32 = 6;
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
        let kind = if arrival { "arrival" } else { "departure" };
        write_sheet(look, arrival, &out_dir.join(format!("{name}-{kind}.png")))?;
    }
    write_strip(look, &out_dir.join(format!("{name}-strip.png")))
}

fn write_sheet(look: Look, arrival: bool, path: &Path) -> io::Result<()> {
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
            let x = if arrival { ARRIVAL_CROP_X } else { 0 };
            blit(&mut crop, frame.as_ref(), -x, -CROP_Y);
            blit(
                &mut sheet,
                crop.as_ref(),
                i32::try_from(index).map_err(io::Error::other)? * CELL_STEP,
                i32::try_from(row).map_err(io::Error::other)? * CELL_STEP,
            );
        }
    }
    save(&sheet, path)
}

fn write_strip(look: Look, path: &Path) -> io::Result<()> {
    // A horizontal edge shows the requested full 1600-pixel span at 1:1.
    let (mut frame, rect, effect) = departure(look, true, 750, Edge::Top);
    let mut timeline = Timeline::default();
    timeline.push(effect, Duration::ZERO);
    for step in if is_stateful(look.style) {
        0..=PREVIEW_FPS
    } else {
        PREVIEW_FPS..=PREVIEW_FPS
    } {
        timeline.render_edge(
            &mut frame,
            Duration::from_secs_f64(f64::from(step) / f64::from(PREVIEW_FPS)),
        );
    }
    let mut strip = pixmap(WIDTH, frame.height)?;
    strip.fill(tiny_skia::Color::from_rgba8(DARK[0], DARK[1], DARK[2], 255));
    overlay(&mut strip, &frame, rect)?;
    save(&strip, path)
}

fn is_stateful(style: Style) -> bool {
    matches!(style, Style::Sparks | Style::Arcs)
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
    let pointer = (f64::from(WIDTH) * 0.4, f64::from(PUSH_Y));
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
    let effect = EdgeEffect {
        look,
        edge,
        progress,
        along: Along::across(area(), rect, pointer, edge, 1.0),
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
        let now = arrival_duration(true) * index / (FRAMES - 1);
        let steps = if is_stateful(look.style) {
            0..=120_u32
        } else {
            120..=120
        };
        for step in steps {
            let at = Duration::from_secs_f64(f64::from(step) / f64::from(PREVIEW_FPS)).min(now);
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
    let crossed = PUSH_STEP * CROSSED_STEP;
    let now = if index < CROSSED_STEP {
        PUSH_STEP * (index + 1)
    } else {
        crossed + DEPARTURE_DURATION * (index - CROSSED_STEP) / 2
    };
    if !is_stateful(look.style) {
        timeline.push(effect, Duration::ZERO);
        if index >= CROSSED_STEP {
            timeline.end(End::Crossed, crossed);
        }
        timeline.render_edge(&mut frame, now);
        return (frame, rect);
    }
    for step in 0..=100_u32 {
        let at = Duration::from_secs_f64(f64::from(step) / f64::from(PREVIEW_FPS)).min(now);
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
