use std::ops::Range;

use anyhow::{Result, ensure};

pub(crate) const ATLAS_DECODED_SIZE: usize = 0xa000;
pub(crate) const TILE_COUNT: usize = 256;
pub(crate) const TILE_WIDTH: usize = 16;
pub(crate) const TILE_HEIGHT: usize = 16;
pub(crate) const PLANE_COUNT: usize = 5;
pub(crate) const BYTES_PER_PLANE: usize = TILE_WIDTH * TILE_HEIGHT / 8;
pub(crate) const BYTES_PER_TILE: usize = BYTES_PER_PLANE * PLANE_COUNT;
#[cfg(feature = "analysis")]
pub(crate) const ATLAS_COLUMNS: usize = 16;
#[cfg(feature = "analysis")]
pub(crate) const ATLAS_ROWS: usize = TILE_COUNT / ATLAS_COLUMNS;
#[cfg(feature = "analysis")]
pub(crate) const ATLAS_WIDTH: usize = ATLAS_COLUMNS * TILE_WIDTH;
#[cfg(feature = "analysis")]
pub(crate) const ATLAS_HEIGHT: usize = ATLAS_ROWS * TILE_HEIGHT;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct MaskedTilePixel {
    #[cfg_attr(not(feature = "analysis"), allow(dead_code))]
    pub(crate) mask: bool,
    pub(crate) color_index: u8,
}

pub(crate) fn plane_range(tile_index: usize, plane_index: usize) -> Result<Range<usize>> {
    ensure!(
        tile_index < TILE_COUNT,
        "masked tile index {tile_index} is outside 0..{TILE_COUNT}"
    );
    ensure!(
        plane_index < PLANE_COUNT,
        "masked tile plane {plane_index} is outside 0..{PLANE_COUNT}"
    );
    let start = tile_index * BYTES_PER_TILE + plane_index * BYTES_PER_PLANE;
    Ok(start..start + BYTES_PER_PLANE)
}

pub(crate) fn decode_pixel(
    atlas: &[u8],
    tile_index: usize,
    x: usize,
    y: usize,
) -> Result<MaskedTilePixel> {
    ensure!(
        atlas.len() == ATLAS_DECODED_SIZE,
        "masked tile atlas must be {ATLAS_DECODED_SIZE} bytes, got {}",
        atlas.len()
    );
    ensure!(x < TILE_WIDTH, "tile x coordinate {x} is out of range");
    ensure!(y < TILE_HEIGHT, "tile y coordinate {y} is out of range");
    let byte_in_plane = y * (TILE_WIDTH / 8) + x / 8;
    let bit = 0x80 >> (x % 8);
    let mask = atlas[plane_range(tile_index, 0)?.start + byte_in_plane] & bit != 0;
    let mut color_index = 0u8;
    for plane_index in 1..PLANE_COUNT {
        if atlas[plane_range(tile_index, plane_index)?.start + byte_in_plane] & bit != 0 {
            color_index |= 1 << (plane_index - 1);
        }
    }
    Ok(MaskedTilePixel { mask, color_index })
}

#[cfg(feature = "analysis")]
pub(crate) fn render_diagnostic_color_ppm(atlas: &[u8]) -> Result<Vec<u8>> {
    render_ppm(atlas, |pixel| diagnostic_rgb(pixel.color_index))
}

#[cfg(feature = "analysis")]
pub(crate) fn render_mask_ppm(atlas: &[u8]) -> Result<Vec<u8>> {
    render_ppm(atlas, |pixel| {
        if pixel.mask {
            [255, 255, 255]
        } else {
            [0, 0, 0]
        }
    })
}

#[cfg(feature = "analysis")]
fn render_ppm(atlas: &[u8], pixel_color: impl Fn(MaskedTilePixel) -> [u8; 3]) -> Result<Vec<u8>> {
    ensure!(
        atlas.len() == ATLAS_DECODED_SIZE,
        "masked tile atlas must be {ATLAS_DECODED_SIZE} bytes, got {}",
        atlas.len()
    );
    let header = format!("P6\n{ATLAS_WIDTH} {ATLAS_HEIGHT}\n255\n");
    let mut ppm = Vec::with_capacity(header.len() + ATLAS_WIDTH * ATLAS_HEIGHT * 3);
    ppm.extend_from_slice(header.as_bytes());
    for canvas_y in 0..ATLAS_HEIGHT {
        for canvas_x in 0..ATLAS_WIDTH {
            let tile_column = canvas_x / TILE_WIDTH;
            let tile_row = canvas_y / TILE_HEIGHT;
            let tile_index = tile_row * ATLAS_COLUMNS + tile_column;
            let pixel = decode_pixel(
                atlas,
                tile_index,
                canvas_x % TILE_WIDTH,
                canvas_y % TILE_HEIGHT,
            )?;
            ppm.extend_from_slice(&pixel_color(pixel));
        }
    }
    Ok(ppm)
}

#[cfg(feature = "analysis")]
fn diagnostic_rgb(color_index: u8) -> [u8; 3] {
    let intensity = if color_index & 0x08 != 0 { 0xff } else { 0x80 };
    let channel = |bit| {
        if color_index & bit != 0 {
            intensity
        } else if color_index & 0x08 != 0 {
            0x40
        } else {
            0x00
        }
    };
    [channel(0x02), channel(0x04), channel(0x01)]
}

#[cfg(all(test, feature = "analysis"))]
#[path = "masked_tile_tests.rs"]
mod tests;
