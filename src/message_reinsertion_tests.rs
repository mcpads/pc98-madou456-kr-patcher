use super::{
    CodebookEntry, SourceProgramBinding, TranslationCodebook, is_static_bios_candidate,
    validate_codebook,
};

#[test]
fn accepts_exactly_the_static_188_cell_candidate_window() {
    let candidates = (0x9f..=0xfc)
        .map(|trail| [0xeb, trail])
        .chain((0x40..=0x7e).map(|trail| [0xec, trail]))
        .chain((0x80..=0x9e).map(|trail| [0xec, trail]))
        .collect::<Vec<_>>();

    assert_eq!(candidates.len(), 188);
    assert!(candidates.into_iter().all(is_static_bios_candidate));
    assert!(!is_static_bios_candidate([0xeb, 0x9e]));
    assert!(!is_static_bios_candidate([0xec, 0x7f]));
    assert!(!is_static_bios_candidate([0xec, 0x9f]));
}

#[test]
fn rejects_duplicate_characters_codes_and_out_of_window_pairs() {
    let mut codebook = fixture();
    assert_eq!(validate_codebook(&codebook).unwrap().len(), 2);

    codebook.entries[1].character = "한".to_owned();
    assert!(validate_codebook(&codebook).is_err());

    let mut codebook = fixture();
    codebook.entries[1].encoded_hex = "eb9f".to_owned();
    assert!(validate_codebook(&codebook).is_err());

    let mut codebook = fixture();
    codebook.entries[1].encoded_hex = "ed40".to_owned();
    assert!(validate_codebook(&codebook).is_err());

    let mut codebook = fixture();
    codebook.entries[1].character = "あ".to_owned();
    assert!(validate_codebook(&codebook).is_err());
}

fn fixture() -> TranslationCodebook {
    TranslationCodebook {
        schema: "pc98_madou456.translation_codebook".to_owned(),
        source_program: SourceProgramBinding {
            name: "MADO456.COM".to_owned(),
            sha256: super::MADO456_SHA256.to_owned(),
        },
        code_space: "static_bios_candidate".to_owned(),
        entries: vec![
            CodebookEntry {
                character: "한".to_owned(),
                encoded_hex: "eb9f".to_owned(),
            },
            CodebookEntry {
                character: "글".to_owned(),
                encoded_hex: "eba0".to_owned(),
            },
        ],
    }
}
