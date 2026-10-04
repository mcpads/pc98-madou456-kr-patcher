use super::*;

fn test_mappings() -> (Vec<[u8; 2]>, Vec<[u8; 2]>) {
    let mut first = vec![[0x81, 0x40]; FIRST_GLYPH_TABLE_ENTRIES];
    first[usize::from(b'A' - 0x20)] = [0x82, 0xa0];
    let second = vec![[0x82, 0xa2]; SECOND_GLYPH_TABLE_ENTRIES];
    (first, second)
}

#[test]
fn decodes_dictionary_glyphs_direct_cp932_and_controls() {
    let (first, second) = test_mappings();
    let source = [0x00, 0x01, b'A', 0xa0, 0x81, 0x75, 0x02, 0x03];
    let (text, tokens) = decode_message_tokens(&source, &first, &second).unwrap();
    assert_eq!(text, "あい「\n");
    assert!(matches!(
        tokens[0],
        MessageToken::Control {
            ref opcode_hex,
            ref argument_hex
        } if opcode_hex == "00" && argument_hex.as_deref() == Some("01")
    ));
    assert!(matches!(tokens[1], MessageToken::Text { .. }));
}

#[test]
fn rejects_missing_control_and_cp932_trail_bytes() {
    let (first, second) = test_mappings();
    assert!(decode_message_tokens(&[0x00], &first, &second).is_err());
    assert!(decode_message_tokens(&[0x81], &first, &second).is_err());
}

#[test]
fn serializes_sparse_message_slots_with_exact_offsets_and_delimiters() {
    let records = [
        MessageRecordBytes {
            slot: 1,
            source: vec![0x11, 0x22],
        },
        MessageRecordBytes {
            slot: 3,
            source: vec![0x33],
        },
    ];
    let bytes = serialize_message_records(&records).unwrap();
    assert_eq!(&bytes[2..4], &513_u16.to_le_bytes());
    assert_eq!(&bytes[6..8], &516_u16.to_le_bytes());
    assert_eq!(&bytes[512..], &[0xff, 0x11, 0x22, 0xff, 0x33, 0xff]);
}

#[test]
fn rejects_duplicate_or_descending_message_slots() {
    for slots in [[1, 1], [2, 1]] {
        let records = slots.map(|slot| MessageRecordBytes {
            slot,
            source: vec![0x11],
        });
        assert!(serialize_message_records(&records).is_err());
    }
}

#[test]
fn summarizes_text_segments_without_flattening_control_positions() {
    let entries = [
        vec![text_token("41"), control_token("02"), text_token("42")],
        vec![
            text_token("43"),
            control_token("00"),
            text_token("44"),
            control_token("03"),
        ],
    ];

    let report = summarize_message_control_layout(entries.iter().map(Vec::as_slice));

    assert_eq!(report.entry_count, 2);
    assert_eq!(report.text_segment_count, 4);
    assert_eq!(report.multi_segment_entry_count, 2);
    assert_eq!(report.max_text_segments_per_entry, 2);
    assert_eq!(report.entries_with_non_line_control_between_text, 1);
    assert_eq!(
        report.control_opcode_counts,
        vec![
            ControlOpcodeCount {
                opcode_hex: "00".to_owned(),
                count: 1,
            },
            ControlOpcodeCount {
                opcode_hex: "02".to_owned(),
                count: 1,
            },
            ControlOpcodeCount {
                opcode_hex: "03".to_owned(),
                count: 1,
            },
        ]
    );
}

#[test]
fn reassembles_changed_text_around_source_owned_controls() {
    let tokens = vec![
        text_token("41"),
        MessageToken::Control {
            opcode_hex: "00".to_owned(),
            argument_hex: Some("07".to_owned()),
        },
        text_token("42"),
        control_token("03"),
    ];
    let translations = ["한".to_owned(), String::new()];

    let reassembled = reassemble_message_tokens(&tokens, &translations, |text| {
        assert_eq!(text, "한");
        Ok(vec![0xeb, 0x9f])
    })
    .unwrap();

    assert_eq!(reassembled, [0xeb, 0x9f, 0x00, 0x07, 0x42, 0x03]);
}

#[test]
fn reassembles_only_authored_message_line_breaks_as_layout_controls() {
    let tokens = vec![text_token("41"), control_token("03")];
    let translation = ["첫 줄\n둘째 줄".to_owned()];
    let reassembled = reassemble_message_tokens(&tokens, &translation, |line| match line {
        "첫 줄" => Ok(vec![0xeb, 0x9f]),
        "둘째 줄" => Ok(vec![0xeb, 0xa0]),
        _ => panic!("unexpected manually authored line"),
    })
    .unwrap();

    assert_eq!(reassembled, [0xeb, 0x9f, 0x02, 0xeb, 0xa0, 0x03]);
}

#[test]
fn rejects_changed_segment_population_and_reserved_replacement_bytes() {
    let tokens = vec![text_token("41")];
    assert!(reassemble_message_tokens(&tokens, &[], |_| Ok(vec![0xeb, 0x9f])).is_err());

    for invalid in [vec![], vec![0x02], vec![0xeb], vec![0xeb, 0x7f]] {
        assert!(
            reassemble_message_tokens(&tokens, &["한".to_owned()], |_| Ok(invalid.clone()))
                .is_err()
        );
    }
}

#[test]
fn encodes_source_table_direct_cp932_and_explicit_external_characters() {
    let (first, second) = test_mappings();
    let encoding = MessageTextEncoding::from_mapping_tables(&first, &second).unwrap();
    let external = BTreeMap::from([('한', [0xeb, 0x9f])]);
    let mut used = BTreeSet::new();

    let encoded = encoding.encode("あ 한！", &external, &mut used).unwrap();

    assert_eq!(encoded, [b'A', 0x20, 0xeb, 0x9f, 0x81, 0x49]);
    assert_eq!(used, BTreeSet::from(['한']));
}

#[test]
fn encodes_runtime_particle_syntax_as_one_reserved_glyph() {
    let (first, second) = test_mappings();
    let encoding = MessageTextEncoding::from_mapping_tables(&first, &second).unwrap();
    let marker = crate::josa::KoreanParticle::Subject.marker();
    let external = BTreeMap::from([(marker, [0xeb, 0x9f])]);
    let mut used = BTreeSet::new();

    let encoded = encoding.encode("{josa:이}", &external, &mut used).unwrap();

    assert_eq!(encoded, [0xeb, 0x9f]);
    assert_eq!(used, BTreeSet::from([marker]));
}

#[test]
fn length_changed_translation_regenerates_following_offsets() {
    let (first, second) = test_mappings();
    let encoding = MessageTextEncoding::from_mapping_tables(&first, &second).unwrap();
    let external = BTreeMap::from([('한', [0xeb, 0x9f]), ('글', [0xeb, 0xa0])]);
    let translated =
        reassemble_message_tokens(&[text_token("41")], &["한글".to_owned()], |text| {
            encoding.encode(text, &external, &mut BTreeSet::new())
        })
        .unwrap();
    let records = [
        MessageRecordBytes {
            slot: 1,
            source: translated,
        },
        MessageRecordBytes {
            slot: 2,
            source: vec![0x42],
        },
    ];

    let bytes = serialize_message_records(&records).unwrap();

    assert_eq!(&bytes[2..4], &513_u16.to_le_bytes());
    assert_eq!(&bytes[4..6], &518_u16.to_le_bytes());
    assert_eq!(
        &bytes[512..],
        &[0xff, 0xeb, 0x9f, 0xeb, 0xa0, 0xff, 0x42, 0xff]
    );
}

#[test]
fn rejects_unmapped_hangul_and_projects_ascii_to_full_width_cells() {
    let (first, second) = test_mappings();
    let encoding = MessageTextEncoding::from_mapping_tables(&first, &second).unwrap();

    assert!(
        encoding
            .encode("한", &BTreeMap::new(), &mut BTreeSet::new())
            .is_err()
    );
    assert_eq!(
        encoding
            .encode("A!?", &BTreeMap::new(), &mut BTreeSet::new())
            .unwrap(),
        [0x82, 0x60, 0x81, 0x49, 0x81, 0x48]
    );
}

fn text_token(source_hex: &str) -> MessageToken {
    MessageToken::Text {
        source_hex: source_hex.to_owned(),
        cp932_hex: source_hex.to_owned(),
        text: "x".to_owned(),
    }
}

fn control_token(opcode_hex: &str) -> MessageToken {
    MessageToken::Control {
        opcode_hex: opcode_hex.to_owned(),
        argument_hex: None,
    }
}
