use std::collections::BTreeMap;

use super::{
    HELD_MONEY_FILE_OFFSETS, HELD_MONEY_SOURCE, Mado456UiTranslation,
    REMAINING_MOVEMENT_COUNT_FILE_OFFSET, REMAINING_MOVEMENT_FILE_OFFSET,
    REMAINING_MOVEMENT_SOURCE,
};

#[test]
#[ignore = "requires assets/translations/mado456-ui-ko.json"]
fn translated_ui_preserves_the_live_count_slot_and_removes_verified_japanese_literals() {
    let translation = Mado456UiTranslation::load().unwrap();
    let mut codebook = BTreeMap::new();
    let mut trail = 0x40_u8;
    for character in translation.required_external_characters() {
        if trail == 0x7f {
            trail += 1;
        }
        codebook.insert(*character, [0xeb, trail]);
        trail += 1;
    }

    let mut source = vec![0_u8; REMAINING_MOVEMENT_FILE_OFFSET + REMAINING_MOVEMENT_SOURCE.len()];
    for offset in HELD_MONEY_FILE_OFFSETS {
        source[offset..offset + HELD_MONEY_SOURCE.len()].copy_from_slice(HELD_MONEY_SOURCE);
    }
    source[REMAINING_MOVEMENT_FILE_OFFSET
        ..REMAINING_MOVEMENT_FILE_OFFSET + REMAINING_MOVEMENT_SOURCE.len()]
        .copy_from_slice(REMAINING_MOVEMENT_SOURCE);

    let (updated, report, used) = translation.apply(&source, &codebook).unwrap();
    assert_eq!(report.entry_count, 2);
    assert_eq!(report.translated_occurrence_count, 5);
    assert_eq!(used, *translation.required_external_characters());
    assert_eq!(
        &updated[REMAINING_MOVEMENT_COUNT_FILE_OFFSET..REMAINING_MOVEMENT_COUNT_FILE_OFFSET + 2],
        &[0x20, 0x20]
    );
    assert!(
        !updated
            .windows(HELD_MONEY_SOURCE.len())
            .any(|bytes| bytes == HELD_MONEY_SOURCE)
    );
    assert!(
        !updated
            .windows(REMAINING_MOVEMENT_SOURCE.len())
            .any(|bytes| bytes == REMAINING_MOVEMENT_SOURCE)
    );
}
