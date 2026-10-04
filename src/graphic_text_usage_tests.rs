use std::collections::BTreeSet;

use super::{hex_tile_index_ranges, parse_tile_map_descriptor};

#[test]
fn parses_row_major_tile_maps_and_excludes_transparent_cells() {
    let bytes = [0xaa, 0xbb, 3, 2, 0x10, 0xff, 0x11, 0x10, 0x12, 0xff];
    let descriptor = parse_tile_map_descriptor(&bytes, 2, Some(0xff)).unwrap();
    assert_eq!(descriptor.width, 3);
    assert_eq!(descriptor.height, 2);
    assert_eq!(descriptor.referenced_cell_count, 4);
    assert_eq!(descriptor.transparent_cell_count, 2);
    assert_eq!(
        descriptor.tile_indexes.into_iter().collect::<Vec<_>>(),
        [0x10, 0x11, 0x12]
    );
}

#[test]
fn rejects_zero_sized_or_truncated_tile_maps() {
    assert!(parse_tile_map_descriptor(&[0, 1], 0, Some(0xff)).is_err());
    assert!(parse_tile_map_descriptor(&[2, 2, 1, 2, 3], 0, Some(0xff)).is_err());
}

#[test]
fn direct_tile_maps_keep_zero_as_a_real_tile() {
    let descriptor = parse_tile_map_descriptor(&[2, 1, 0, 1], 0, None).unwrap();
    assert_eq!(descriptor.transparent_cell_count, 0);
    assert_eq!(
        descriptor.tile_indexes.into_iter().collect::<Vec<_>>(),
        [0, 1]
    );
}

#[test]
fn reports_tile_indexes_as_compact_sorted_ranges() {
    let indexes = BTreeSet::from([0x01, 0x02, 0x03, 0x05, 0xfe, 0xff]);
    assert_eq!(hex_tile_index_ranges(&indexes), ["01-03", "05", "fe-ff"]);
}
