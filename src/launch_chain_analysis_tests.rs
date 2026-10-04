use super::{
    DRSHELL_CHILD_LOOP_FILE_OFFSET, DRSHELL_DISPATCH_FILE_OFFSET, DRSHELL_EXEC_FILE_OFFSET,
    DRSHELL_EXEC_PARAMETER_BLOCK_FILE_OFFSET, DRSHELL_FILE_SIZE,
    DRSHELL_IMAGE_END_LITERAL_FILE_OFFSET, DRSHELL_NEXT_SELECTOR_QUERY_FILE_OFFSET,
    DRSHELL_SELECTOR_TABLE_FILE_OFFSET, analyze_shell_dispatch,
    count_immediate_int18_ah1a_sequences,
};

#[test]
fn parses_every_shell_selector_and_preserves_the_shared_ending_state() {
    let shell = shell_fixture();
    let report = analyze_shell_dispatch(&shell).unwrap();
    let programs = report
        .selectors
        .iter()
        .map(|selector| selector.program.as_str())
        .collect::<Vec<_>>();

    assert_eq!(
        programs,
        [
            "OPENING.COM",
            "MDSC.COM",
            "MADO456.COM",
            "ENDING.COM",
            "ENDING.COM"
        ]
    );
    assert_eq!(report.initial_selector, 0);
    assert_eq!(report.allocation_paragraphs_before_child_exec, 0x5f);
    assert_eq!(report.zero_filled_file_tail_bytes, 270);
}

#[test]
fn rejects_a_changed_shell_selector_target() {
    let mut shell = shell_fixture();
    shell[0x02a4] = b'X';
    assert!(analyze_shell_dispatch(&shell).is_err());
}

#[test]
fn counts_only_the_two_immediate_bios_registration_forms() {
    let bytes = [
        0xb4, 0x1a, 0xcd, 0x18, 0xb8, 0x00, 0x1a, 0xcd, 0x18, 0xb4, 0x1a, 0x90, 0xcd, 0x18,
    ];
    assert_eq!(count_immediate_int18_ah1a_sequences(&bytes), 2);
}

fn shell_fixture() -> Vec<u8> {
    let mut shell = vec![0; DRSHELL_FILE_SIZE];
    write(
        &mut shell,
        DRSHELL_CHILD_LOOP_FILE_OFFSET,
        &[0x32, 0xc0, 0xe8, 0x09, 0x00, 0x3c, 0xff, 0x75, 0xf9],
    );
    write(
        &mut shell,
        DRSHELL_DISPATCH_FILE_OFFSET,
        &[0x8a, 0xd8, 0x8c, 0xc8, 0x8e, 0xd8, 0x8e, 0xc0],
    );
    write(
        &mut shell,
        DRSHELL_EXEC_FILE_OFFSET,
        &[0xb8, 0x00, 0x4b, 0xcd, 0x21],
    );
    write(
        &mut shell,
        DRSHELL_NEXT_SELECTOR_QUERY_FILE_OFFSET,
        &[0xb4, 0x03, 0xcd, 0x64],
    );
    write(
        &mut shell,
        DRSHELL_IMAGE_END_LITERAL_FILE_OFFSET - 1,
        &[0xbb, 0xe8, 0x04, 0xc1, 0xeb, 0x04, 0x83, 0xc3, 0x11],
    );
    write(
        &mut shell,
        DRSHELL_SELECTOR_TABLE_FILE_OFFSET,
        &[0x90, 0x03, 0x95, 0x03, 0x9a, 0x03, 0x9f, 0x03, 0x9f, 0x03],
    );
    write(&mut shell, 0x0290, &[1, 0xa4, 0x03, 0xb1, 0x03]);
    write(&mut shell, 0x0295, &[1, 0xb4, 0x03, 0xbe, 0x03]);
    write(&mut shell, 0x029a, &[1, 0xbf, 0x03, 0xcc, 0x03]);
    write(&mut shell, 0x029f, &[1, 0xcd, 0x03, 0xd9, 0x03]);
    write(&mut shell, 0x02a4, b"opening.com\0");
    write(&mut shell, 0x02b4, b"mdsc.com\0");
    write(&mut shell, 0x02bf, b"mado456.com\0");
    write(&mut shell, 0x02cd, b"ending.com\0");
    shell[DRSHELL_EXEC_PARAMETER_BLOCK_FILE_OFFSET..].fill(0);
    shell
}

fn write(output: &mut [u8], offset: usize, bytes: &[u8]) {
    output[offset..offset + bytes.len()].copy_from_slice(bytes);
}
