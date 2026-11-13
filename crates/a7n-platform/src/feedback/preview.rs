// SPDX-License-Identifier: GPL-3.0-or-later

//! Desktop-free PNG contact sheets using the native feedback renderer.

use std::fs::{self, File};
use std::io::{self, BufWriter};
use std::path::Path;
use std::time::Duration;

use a7n_proto::v1::Edge;
use tiny_skia::{Pixmap, PixmapPaint, PixmapRef, Transform};

use super::look::{self, BEAM_THICKNESS, Rect};
use super::raster::{self, Along, Arrival, EdgeEffect, Frame};
use super::timeline::{End, FADE_DURATION, PLAY_DURATION, Timeline};
use super::{Color, Landing, Look, Size, Style};

const WIDTH: u32 = 1600;
const HEIGHT: u32 = 1000;
const PUSH_Y: u32 = HEIGHT * 2 / 5;
const CROP: u32 = 360;
const GAP: u32 = 4;
const FRAMES: u32 = 8;
const DARK: [u8; 3] = [0x0e, 0x11, 0x16];
const LIGHT: [u8; 3] = [0xf4, 0xf5, 0xf7];
const PUSH_PROGRESS: [u16; 5] = [0, 250, 500, 750, 950];
const PUSH_STEP: Duration = Duration::from_millis(200);
const GLOW_DEPTH: f64 = 6.0;
const LANDING_DIAMETER: f64 = 24.0;
const LANDING_STROKE: f64 = 2.0;

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
    timeline.render_edge(&mut frame, Duration::ZERO);
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
fn frame(rect: Rect, look: Look, dark: bool) -> Frame {
    let mut frame = Frame {
        animate: true,
        dark,
        opacity: 1.0,
        stroke: LANDING_STROKE * look.size.multiplier(),
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
    let depth = if look.style.animated() {
        f64::from(raster::edge_depth(look.style))
    } else if look.style == Style::Beam {
        BEAM_THICKNESS
    } else {
        GLOW_DEPTH
    };
    let point = if horizontal { (push, 0.0) } else { (0.0, push) };
    let rect = look::beam_rect(
        area(),
        edge,
        point,
        if look.style.animated() {
            1000
        } else {
            progress
        },
        depth * look.size.multiplier(),
    )
    .expect("fixed preview monitor and specified edge have valid geometry");
    let start = if horizontal { rect.x } else { rect.y };
    let effect = EdgeEffect {
        look,
        edge,
        progress,
        along: Along {
            start,
            span: f64::from(span),
            push: push - start,
        },
    };
    (frame(rect, look, dark), rect, effect)
}

fn snapshot(look: Look, dark: bool, index: u32, arrival: bool) -> io::Result<Pixmap> {
    let mut timeline = Timeline::default();
    let (frame, rect) = if arrival {
        let point = (f64::from(WIDTH - 1), f64::from(PUSH_Y));
        let diameter = if look.style.animated() {
            f64::from(raster::arrival_size(look.style))
        } else {
            LANDING_DIAMETER
        } * look.size.multiplier();
        let rect = look::landing_rect(area(), point, diameter, look.style);
        let mut frame = frame(rect, look, dark);
        frame.arrival = Some(Arrival::at(area(), rect, point, 1.0));
        timeline.land(look, Duration::ZERO);
        timeline.render_landing(&mut frame, PLAY_DURATION * index / (FRAMES - 1));
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
    for (step, progress) in PUSH_PROGRESS
        .into_iter()
        .enumerate()
        .take(index as usize + 1)
    {
        effect.progress = progress;
        timeline.push(
            effect,
            PUSH_STEP * u32::try_from(step).expect("five push samples fit u32"),
        );
    }
    let now = if index < 5 {
        PUSH_STEP * index
    } else {
        let crossed = PUSH_STEP * 5;
        timeline.end(End::Crossed, crossed);
        let duration = if look.style.animated() {
            PLAY_DURATION
        } else {
            FADE_DURATION
        };
        crossed + duration * (index - 5) / 2
    };
    timeline.render_edge(&mut frame, now);
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
