// SPDX-License-Identifier: GPL-3.0-or-later

use std::io::Cursor;

use a7n_core::clipboard::validate_png;
use a7n_proto::limits;

const INFO_HEADER_SIZE: u32 = 40;
const INFO_HEADER: usize = INFO_HEADER_SIZE as usize;
const V5_HEADER: usize = 124;
const MAX_PALETTE_COLORS: usize = 256;
pub(super) const MAX_DIB_BYTES: usize =
    limits::IMAGE_DECODED_MAX + V5_HEADER + MAX_PALETTE_COLORS * 4;
const BI_RGB: u32 = 0;
const BI_BITFIELDS: u32 = 3;

fn word(bytes: &[u8], offset: usize) -> Option<u16> {
    Some(u16::from_le_bytes(
        bytes.get(offset..offset + 2)?.try_into().ok()?,
    ))
}

fn dword(bytes: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_le_bytes(
        bytes.get(offset..offset + 4)?.try_into().ok()?,
    ))
}

fn dimensions(width: u32, height: u32) -> Option<usize> {
    if width == 0
        || height == 0
        || width > limits::IMAGE_MAX_DIMENSION
        || height > limits::IMAGE_MAX_DIMENSION
    {
        return None;
    }
    let size = u64::from(width) * u64::from(height) * 4;
    (size <= limits::IMAGE_DECODED_MAX as u64)
        .then(|| usize::try_from(size).ok())
        .flatten()
}

fn channel(pixel: u32, mask: u32) -> Option<u8> {
    if mask == 0 {
        return None;
    }
    let value = (pixel & mask) >> mask.trailing_zeros();
    let maximum = mask >> mask.trailing_zeros();
    // Reject non-contiguous bitfields rather than interpreting ambiguous pixels.
    if maximum & maximum.wrapping_add(1) != 0 {
        return None;
    }
    u8::try_from(u64::from(value) * 255 / u64::from(maximum)).ok()
}

pub(super) fn from_dib(bytes: &[u8]) -> Option<Vec<u8>> {
    if bytes.len() > MAX_DIB_BYTES {
        return None;
    }
    let header = usize::try_from(dword(bytes, 0)?).ok()?;
    if ![INFO_HEADER, 52, 56, 108, 124].contains(&header) || bytes.len() < header {
        return None;
    }
    let width = i32::from_le_bytes(bytes.get(4..8)?.try_into().ok()?);
    let signed_height = i32::from_le_bytes(bytes.get(8..12)?.try_into().ok()?);
    let width = u32::try_from(width).ok()?;
    let height = signed_height.unsigned_abs();
    let decoded = dimensions(width, height)?;
    let depth = word(bytes, 14)?;
    let compression = dword(bytes, 16)?;
    // shortcut: compressed DIBs are rejected; add native conversion if a clipboard producer needs them.
    if word(bytes, 12)? != 1
        || ![1, 4, 8, 16, 24, 32].contains(&depth)
        || ![BI_RGB, BI_BITFIELDS].contains(&compression)
        || (compression == BI_BITFIELDS && ![16, 32].contains(&depth))
    {
        return None;
    }
    let palette_len = if depth <= 8 {
        let used = dword(bytes, 32)?;
        if used > 1 << depth {
            return None;
        }
        if used == 0 {
            1_usize << depth
        } else {
            usize::try_from(used).ok()?
        }
    } else {
        usize::try_from(dword(bytes, 32)?).ok()?
    };
    let external_masks = if compression == BI_BITFIELDS && header == INFO_HEADER {
        12
    } else {
        0
    };
    let palette_start = header.checked_add(external_masks)?;
    let start = palette_start.checked_add(palette_len.checked_mul(4)?)?;
    let stride = (usize::try_from(width).ok()? * usize::from(depth)).div_ceil(32) * 4;
    let pixels = bytes.get(start..start.checked_add(stride.checked_mul(height as usize)?)?)?;
    let masks = if compression == BI_BITFIELDS {
        [dword(bytes, 40)?, dword(bytes, 44)?, dword(bytes, 48)?]
    } else if depth == 16 {
        [0x7c00, 0x03e0, 0x001f]
    } else {
        [0x00ff_0000, 0xff00, 0xff]
    };
    if depth > 8
        && (masks
            .iter()
            .any(|mask| *mask == 0 || (depth < 32 && *mask >> depth != 0))
            || masks[0] & masks[1] != 0
            || masks[0] & masks[2] != 0
            || masks[1] & masks[2] != 0)
    {
        return None;
    }
    let alpha_mask = if compression == BI_BITFIELDS && header >= 56 {
        dword(bytes, 52)?
    } else {
        0
    };
    if (alpha_mask != 0 && depth != 32) || alpha_mask & (masks[0] | masks[1] | masks[2]) != 0 {
        return None;
    }
    let mut rgba = vec![0; decoded];
    for y in 0..height as usize {
        let source_y = if signed_height < 0 {
            y
        } else {
            height as usize - 1 - y
        };
        let row = &pixels[source_y * stride..(source_y + 1) * stride];
        for x in 0..width as usize {
            let rgb = dib_pixel(row, x, depth, &bytes[palette_start..start], masks)?;
            let offset = (y * width as usize + x) * 4;
            rgba[offset..offset + 3].copy_from_slice(&rgb);
            rgba[offset + 3] = if alpha_mask == 0 {
                255
            } else {
                channel(dword(row, x * 4)?, alpha_mask)?
            };
        }
    }
    encode_png(&rgba, width, height)
}

fn dib_pixel(
    row: &[u8],
    column: usize,
    depth: u16,
    palette: &[u8],
    masks: [u32; 3],
) -> Option<[u8; 3]> {
    if depth <= 8 {
        let bit = column * usize::from(depth);
        let index = usize::from(
            (row[bit / 8] >> (8 - usize::from(depth) - bit % 8))
                & u8::try_from((1_u16 << depth) - 1).ok()?,
        );
        let color = palette.get(index * 4..index * 4 + 4)?;
        return Some([color[2], color[1], color[0]]);
    }
    let size = usize::from(depth / 8);
    let offset = column * size;
    let mut value = [0; 4];
    value[..size].copy_from_slice(row.get(offset..offset + size)?);
    let pixel = u32::from_le_bytes(value);
    Some([
        channel(pixel, masks[0])?,
        channel(pixel, masks[1])?,
        channel(pixel, masks[2])?,
    ])
}

fn encode_png(rgba: &[u8], width: u32, height: u32) -> Option<Vec<u8>> {
    let mut output = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut output, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.write_header().ok()?.write_image_data(rgba).ok()?;
    }
    (output.len() <= limits::IMAGE_CLIPBOARD_MAX).then_some(output)
}

pub(super) fn to_dib(bytes: &[u8]) -> Option<Vec<u8>> {
    let (width, height) = validate_png(bytes).ok()?;
    let mut decoder = png::Decoder::new_with_limits(
        Cursor::new(bytes),
        png::Limits {
            bytes: limits::IMAGE_DECODED_MAX,
        },
    );
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().ok()?;
    let size = reader.output_buffer_size()?;
    if size > limits::IMAGE_DECODED_MAX {
        return None;
    }
    let mut pixels = vec![0; size];
    let info = reader.next_frame(&mut pixels).ok()?;
    let mut dib = vec![0; INFO_HEADER + dimensions(width, height)?];
    dib[..4].copy_from_slice(&INFO_HEADER_SIZE.to_le_bytes());
    dib[4..8].copy_from_slice(&width.to_le_bytes());
    dib[8..12].copy_from_slice(&height.to_le_bytes());
    dib[12..14].copy_from_slice(&1_u16.to_le_bytes());
    dib[14..16].copy_from_slice(&32_u16.to_le_bytes());
    let samples = info.color_type.samples();
    for (index, source) in pixels[..info.buffer_size()]
        .chunks_exact(samples)
        .enumerate()
    {
        let (red, green, blue) = match info.color_type {
            png::ColorType::Rgb | png::ColorType::Rgba => (source[0], source[1], source[2]),
            png::ColorType::Grayscale | png::ColorType::GrayscaleAlpha => {
                (source[0], source[0], source[0])
            }
            png::ColorType::Indexed => return None,
        };
        let x = index % width as usize;
        let y = index / width as usize;
        let offset = INFO_HEADER + ((height as usize - 1 - y) * width as usize + x) * 4;
        dib[offset..offset + 4].copy_from_slice(&[blue, green, red, 0]);
    }
    Some(dib)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn convert_dib_round_trip_preserves_pixels_and_rejects_truncation() {
        // Arrange
        let mut dib = vec![0; INFO_HEADER + 4];
        dib[..4].copy_from_slice(&INFO_HEADER_SIZE.to_le_bytes());
        dib[4..8].copy_from_slice(&1_u32.to_le_bytes());
        dib[8..12].copy_from_slice(&1_u32.to_le_bytes());
        dib[12..14].copy_from_slice(&1_u16.to_le_bytes());
        dib[14..16].copy_from_slice(&32_u16.to_le_bytes());
        dib[INFO_HEADER..].copy_from_slice(&[3, 2, 1, 0]);

        // Act
        let png = from_dib(&dib).unwrap();
        let restored = to_dib(&png).unwrap();
        let truncated = from_dib(&dib[..INFO_HEADER]);

        // Assert
        assert_eq!(restored, dib);
        assert!(truncated.is_none());
        assert!(to_dib(b"invalid PNG").is_none());
    }

    #[test]
    fn from_dib_palette_bgr_and_bitfields_preserves_colors() {
        // Arrange
        let cases = [
            (1_u16, INFO_HEADER, BI_RGB),
            (24, INFO_HEADER, BI_RGB),
            (32, V5_HEADER, BI_BITFIELDS),
        ];
        for (depth, header, compression) in cases {
            let palette = if depth == 1 { 8 } else { 0 };
            let mut dib = vec![0; header + palette + 4];
            dib[..4].copy_from_slice(&u32::try_from(header).unwrap().to_le_bytes());
            dib[4..8].copy_from_slice(&1_u32.to_le_bytes());
            dib[8..12].copy_from_slice(&(-1_i32).to_le_bytes());
            dib[12..14].copy_from_slice(&1_u16.to_le_bytes());
            dib[14..16].copy_from_slice(&depth.to_le_bytes());
            dib[16..20].copy_from_slice(&compression.to_le_bytes());
            let expected = if depth == 1 {
                dib[header + 4..header + 8].copy_from_slice(&[3, 2, 1, 0]);
                dib[header + palette] = 0x80;
                [1, 2, 3, 255]
            } else {
                dib[header..header + 4].copy_from_slice(&[3, 2, 1, 128]);
                if compression == BI_BITFIELDS {
                    for (index, mask) in [0x00ff_0000_u32, 0xff00, 0xff, 0xff00_0000]
                        .into_iter()
                        .enumerate()
                    {
                        dib[INFO_HEADER + index * 4..INFO_HEADER + index * 4 + 4]
                            .copy_from_slice(&mask.to_le_bytes());
                    }
                    [1, 2, 3, 128]
                } else {
                    [1, 2, 3, 255]
                }
            };

            // Act
            let encoded = from_dib(&dib).unwrap();
            let mut reader = png::Decoder::new(Cursor::new(encoded)).read_info().unwrap();
            let mut decoded = [0; 4];
            reader.next_frame(&mut decoded).unwrap();

            // Assert
            assert_eq!(decoded, expected);
        }
    }

    #[test]
    fn from_dib_unsafe_dimensions_rejects_before_allocating_pixels() {
        // Arrange
        let mut dib = vec![0; INFO_HEADER];
        dib[..4].copy_from_slice(&INFO_HEADER_SIZE.to_le_bytes());
        dib[4..8].copy_from_slice(&limits::IMAGE_MAX_DIMENSION.to_le_bytes());
        dib[8..12].copy_from_slice(&limits::IMAGE_MAX_DIMENSION.to_le_bytes());
        dib[12..14].copy_from_slice(&1_u16.to_le_bytes());
        dib[14..16].copy_from_slice(&32_u16.to_le_bytes());

        // Act
        let result = from_dib(&dib);

        // Assert
        assert!(result.is_none());
    }
}
