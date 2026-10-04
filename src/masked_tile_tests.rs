use super::{
    ATLAS_DECODED_SIZE, ATLAS_HEIGHT, ATLAS_WIDTH, BYTES_PER_PLANE, BYTES_PER_TILE,
    MaskedTilePixel, PLANE_COUNT, TILE_COUNT, decode_pixel, plane_range,
    render_diagnostic_color_ppm, render_mask_ppm,
};

#[test]
fn maps_interleaved_tile_planes_without_crossing_the_atlas() {
    assert_eq!(plane_range(0, 0).unwrap(), 0..32);
    assert_eq!(
        plane_range(1, 0).unwrap(),
        BYTES_PER_TILE..BYTES_PER_TILE + BYTES_PER_PLANE
    );
    assert_eq!(
        plane_range(TILE_COUNT - 1, PLANE_COUNT - 1).unwrap(),
        ATLAS_DECODED_SIZE - BYTES_PER_PLANE..ATLAS_DECODED_SIZE
    );
    assert!(plane_range(TILE_COUNT, 0).is_err());
    assert!(plane_range(0, PLANE_COUNT).is_err());
}

#[test]
fn decodes_mask_and_four_color_bits_most_significant_pixel_first() {
    let mut atlas = vec![0u8; ATLAS_DECODED_SIZE];
    atlas[plane_range(0, 0).unwrap().start] = 0x80;
    atlas[plane_range(0, 1).unwrap().start] = 0x80;
    atlas[plane_range(0, 3).unwrap().start] = 0x80;
    assert_eq!(
        decode_pixel(&atlas, 0, 0, 0).unwrap(),
        MaskedTilePixel {
            mask: true,
            color_index: 0x05,
        }
    );
    assert_eq!(
        decode_pixel(&atlas, 0, 1, 0).unwrap(),
        MaskedTilePixel {
            mask: false,
            color_index: 0,
        }
    );
}

#[test]
fn renders_a_complete_binary_ppm_for_each_atlas_plane_view() {
    let atlas = vec![0u8; ATLAS_DECODED_SIZE];
    let expected_header = format!("P6\n{ATLAS_WIDTH} {ATLAS_HEIGHT}\n255\n");
    let expected_size = expected_header.len() + ATLAS_WIDTH * ATLAS_HEIGHT * 3;
    for rendered in [
        render_diagnostic_color_ppm(&atlas).unwrap(),
        render_mask_ppm(&atlas).unwrap(),
    ] {
        assert!(rendered.starts_with(expected_header.as_bytes()));
        assert_eq!(rendered.len(), expected_size);
        assert!(
            rendered[expected_header.len()..]
                .iter()
                .all(|byte| *byte == 0)
        );
    }
}
