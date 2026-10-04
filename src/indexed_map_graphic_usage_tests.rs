use std::collections::BTreeSet;

use super::{parse_indexed_tile_map_descriptor, parse_map_descriptor_selector_table};

#[test]
fn parses_indexed_tile_maps_without_publishing_cell_order() {
    let descriptor =
        parse_indexed_tile_map_descriptor(&[2, 0, 1, 0, 0x01, 0x02, 0x03, 0x00], 0).unwrap();
    assert_eq!(descriptor.width, 2);
    assert_eq!(descriptor.height, 1);
    assert_eq!(descriptor.tile_indexes, BTreeSet::from([0x01, 0x03]));
    assert_eq!(descriptor.cell_high_bytes, BTreeSet::from([0x00, 0x02]));
    assert!(parse_indexed_tile_map_descriptor(&[0, 0, 1, 0], 0).is_err());
    assert!(parse_indexed_tile_map_descriptor(&[2, 0, 1, 0, 1, 0], 0).is_err());
}

#[test]
fn parses_map_state_records_with_shared_tails_and_requires_terminators() {
    let bytes = [
        0x20, 0x01, 0x30, 0x00, 0xff, 0xff, 0xff, 0xff, 0x00, 0x00, 0x00, 0x00,
    ];
    assert_eq!(
        parse_map_descriptor_selector_table(&bytes, 0x100, 6, 2).unwrap(),
        [vec![0x120], vec![]]
    );
    assert!(parse_map_descriptor_selector_table(&bytes[..4], 0x100, 4, 1).is_err());

    let shared_tail = [
        0x20, 0x01, 0x30, 0x00, 0x21, 0x01, 0x31, 0x00, 0xff, 0xff, 0x00, 0x00,
    ];
    assert_eq!(
        parse_map_descriptor_selector_table(&shared_tail, 0x100, 4, 3).unwrap(),
        [vec![0x120, 0x121], vec![0x121], vec![]]
    );
}
