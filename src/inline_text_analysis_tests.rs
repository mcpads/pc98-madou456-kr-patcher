use super::*;

#[test]
fn inline_translation_projects_ascii_to_two_byte_cells() {
    let codebook = BTreeMap::from([('한', [0xeb, 0x40])]);
    let mut used = BTreeSet::new();

    let encoded = encode_two_byte_renderer_text("한 A!?", &codebook, &mut used).unwrap();

    assert_eq!(encoded.len(), 10);
    assert_eq!(&encoded[..2], &[0xeb, 0x40]);
    assert_eq!(used, BTreeSet::from(['한']));
    assert_eq!(SHIFT_JIS.decode(&encoded[2..]).0, "　Ａ！？");
}

#[test]
fn translated_inline_pool_terminates_each_two_byte_cell_stream_with_a_zero_word() {
    let mut pool = Vec::new();

    append_translated_inline_string(&mut pool, &[0xeb, 0x40]);
    append_translated_inline_string(&mut pool, &[0xeb, 0x41]);

    assert_eq!(pool, [0xeb, 0x40, 0, 0, 0xeb, 0x41, 0, 0]);
}

fn inline_text_test_profile() -> InlineTextBlockProfile {
    InlineTextBlockProfile {
        id: "test_text",
        role: "test text",
        program: "TEST.COM",
        program_sha256: "unused",
        descriptor_table_file_offset: 0x10,
        entry_count: 2,
        consumer_file_offset: 0,
    }
}

fn inline_text_test_program() -> Vec<u8> {
    let mut program = vec![0; 0x40];
    program[0x11..0x13].copy_from_slice(&0x120_u16.to_le_bytes());
    program[0x19..0x1b].copy_from_slice(&0x122_u16.to_le_bytes());
    program[0x20..0x22].copy_from_slice(b"A\0");
    program[0x22..0x25].copy_from_slice(&[0x82, 0xa0, 0]);
    program
}

#[test]
fn extracts_inline_text_only_through_ordered_descriptors() {
    let (summary, entries) =
        parse_inline_text_block(&inline_text_test_program(), inline_text_test_profile()).unwrap();
    assert_eq!(summary.entry_count, 2);
    assert_eq!(summary.source_byte_count, 3);
    assert_eq!(summary.strict_cp932_entry_count, 2);
    assert_eq!(entries[0].decoded_text_lossy, "A");
    assert_eq!(entries[1].decoded_text_lossy, "あ");
}

#[test]
fn rejects_inline_text_descriptors_that_reuse_an_earlier_string() {
    let mut program = inline_text_test_program();
    program[0x19..0x1b].copy_from_slice(&0x120_u16.to_le_bytes());
    assert!(parse_inline_text_block(&program, inline_text_test_profile()).is_err());
}

fn inline_text_test_block() -> InlineTextBlockCatalog {
    InlineTextBlockCatalog {
        id: "test_text",
        entries: [(&b"A"[..], 0x10), (&b"BC"[..], 0x18)]
            .into_iter()
            .enumerate()
            .map(
                |(descriptor_index, (source, descriptor_file_offset))| InlineTextEntry {
                    id: format!("test_text:{descriptor_index:03}"),
                    descriptor_index,
                    descriptor_file_offset,
                    text_file_offset: 0,
                    text_runtime_address: 0,
                    source: source.to_vec(),
                    source_hex: hex_bytes(source),
                    decoded_text_lossy: String::new(),
                    strict_cp932: true,
                },
            )
            .collect(),
    }
}

fn inline_text_test_arena(file_end: usize) -> InlineTextArenaProfile {
    InlineTextArenaProfile {
        id: "test_pool",
        block_id: "test_text",
        program: "TEST.COM",
        first_entry: 0,
        entry_count: 2,
        file_start: 0x20,
        file_end,
        source_leading_zero_count: 1,
        source_separator_zero_count: 1,
    }
}

#[test]
fn relocates_inline_text_and_updates_descriptor_pointers() {
    let mut program = vec![0xaa; 0x40];
    write_inline_text_arena(
        &mut program,
        &inline_text_test_block(),
        inline_text_test_arena(0x30),
    )
    .unwrap();

    assert_eq!(&program[0x11..0x13], &0x121_u16.to_le_bytes());
    assert_eq!(&program[0x19..0x1b], &0x124_u16.to_le_bytes());
    assert_eq!(&program[0x20..0x27], b"\0A\0\0BC\0");
    assert!(program[0x27..0x30].iter().all(|byte| *byte == 0));
    assert_eq!(program[0x30], 0xaa);
}

#[test]
fn rejects_inline_text_that_exceeds_its_storage_arena() {
    let mut program = vec![0xaa; 0x40];
    assert!(
        write_inline_text_arena(
            &mut program,
            &inline_text_test_block(),
            inline_text_test_arena(0x26),
        )
        .is_err()
    );
}
