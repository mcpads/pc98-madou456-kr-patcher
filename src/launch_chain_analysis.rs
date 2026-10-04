use anyhow::{Context, Result, ensure};
use serde::Serialize;

use crate::source_cd::GameFile;

use super::{
    COM_LOAD_ORIGIN, DRBIOS_SHA256, hex_bytes, read_u16_le, require_file, require_sha256,
    text_renderer_analysis::{CG_GLYPH_BYTES, EXTERNAL_CHARACTER_CANDIDATE_CAPACITY},
};

const SHORT_LAUNCHER_SHA256: &str =
    "046d84899778b2ee4b7617cb57a6d58f70be5aaeb1684dfa4d59269f9796f02a";
const LONG_LAUNCHER_SHA256: &str =
    "7630f2cb07da3b90bad8fbf286567541cbc5037bf374a23c8b23b2856d77ed7f";
const DRSHELL_SHA256: &str = "3ff9af018a3f0a33fff7f3b94fe4bd8ebefc7aed34a63baafea1bbc351bfc6ef";

const SHORT_LAUNCHER_BYTES: &[u8] = b"@echo off\r\n\
fplay /b44\r\n\
drbios\r\n\
drshell\r\n\
drbios /r\r\n\
fplay\r\n\
\x1a";
const LONG_LAUNCHER_BYTES: &[u8] = b"echo off\r\n\
FPLAY.COM /b44\r\n\
DRBIOS.COM\r\n\
DRSHELL.COM\r\n\
DRBIOS.COM /r\r\n\
FPLAY.COM\r\n";

const DRSHELL_CHILD_LOOP_FILE_OFFSET: usize = 0x0069;
const DRSHELL_DISPATCH_FILE_OFFSET: usize = 0x0077;
const DRSHELL_EXEC_FILE_OFFSET: usize = 0x00b2;
const DRSHELL_NEXT_SELECTOR_QUERY_FILE_OFFSET: usize = 0x00be;
const DRSHELL_IMAGE_END_LITERAL_FILE_OFFSET: usize = 0x001d;
const DRSHELL_SELECTOR_TABLE_FILE_OFFSET: usize = 0x0286;
const DRSHELL_SELECTOR_COUNT: usize = 5;
const DRSHELL_EXEC_PARAMETER_BLOCK_FILE_OFFSET: usize = 0x02da;
const DRSHELL_FILE_SIZE: usize = 1_000;

const DRBIOS_ENTRY_FILE_OFFSET: usize = 0x2390;
const DRBIOS_TSR_FILE_OFFSET: usize = 0x245f;
const DRBIOS_REMOVE_PATH_FILE_OFFSET: usize = 0x2465;
const DRBIOS_INT64_RESTORE_FILE_OFFSET: usize = 0x24e1;
const DRBIOS_RESIDENT_BLOCK_RELEASE_FILE_OFFSET: usize = 0x2527;
const DRBIOS_INT64_INSTALL_FILE_OFFSET: usize = 0x25c0;

const IMMEDIATE_GAIJI_SCAN_PROGRAMS: [&str; 7] = [
    "DRBIOS.COM",
    "DRSHELL.COM",
    "ENDING.COM",
    "FPLAY.COM",
    "MADO456.COM",
    "MDSC.COM",
    "OPENING.COM",
];

#[derive(Debug, Serialize)]
pub(super) struct LaunchChainAnalysis {
    launcher_scripts: Vec<LauncherScript>,
    resident_driver_lifetime: ResidentDriverLifetime,
    shell_dispatch: ShellDispatch,
    external_character_installation_candidate: ExternalCharacterInstallationCandidate,
}

#[derive(Debug, Serialize)]
struct LauncherScript {
    name: &'static str,
    sha256: &'static str,
    normalized_command_sequence: Vec<&'static str>,
}

#[derive(Debug, Serialize)]
struct ResidentDriverLifetime {
    program: ProgramIdentity,
    entry_file_offset: usize,
    int64_install_file_offset: usize,
    terminate_and_stay_resident_file_offset: usize,
    remove_path_file_offset: usize,
    int64_restore_file_offset: usize,
    resident_block_release_file_offset: usize,
    observation: &'static str,
}

#[derive(Debug, Serialize)]
struct ShellDispatch {
    program: ProgramIdentity,
    child_loop_file_offset: usize,
    dispatch_file_offset: usize,
    child_exec_file_offset: usize,
    next_selector_query_file_offset: usize,
    selector_table_file_offset: usize,
    initial_selector: usize,
    selectors: Vec<ShellSelector>,
    allocation_paragraphs_before_child_exec: usize,
    zero_filled_file_tail_offset: usize,
    zero_filled_file_tail_bytes: usize,
    exec_parameter_block_file_offset: usize,
    observation: &'static str,
}

#[derive(Debug, Serialize)]
struct ShellSelector {
    selector: usize,
    record_file_offset: usize,
    program: String,
    filename_file_offset: usize,
}

#[derive(Debug, Serialize)]
struct ProgramIdentity {
    name: &'static str,
    sha256: &'static str,
}

#[derive(Debug, Serialize)]
struct ExternalCharacterInstallationCandidate {
    status: &'static str,
    common_static_boundary: &'static str,
    text_consumer_programs: Vec<&'static str>,
    candidate_glyph_capacity: usize,
    observed_bytes_per_cg_glyph: usize,
    full_window_raw_bitmap_bytes: usize,
    drshell_zero_filled_file_tail_bytes: usize,
    full_window_fits_in_entire_zero_filled_file_tail: bool,
    scanned_programs: Vec<&'static str>,
    immediate_int18_ah1a_sequence_count: usize,
    observation: &'static str,
    limitation: &'static str,
}

pub(super) fn analyze_launch_chain(files: &[GameFile]) -> Result<LaunchChainAnalysis> {
    verify_launcher(
        files,
        "456.BAT",
        SHORT_LAUNCHER_SHA256,
        SHORT_LAUNCHER_BYTES,
    )?;
    verify_launcher(
        files,
        "MADOU456.BAT",
        LONG_LAUNCHER_SHA256,
        LONG_LAUNCHER_BYTES,
    )?;

    let drbios = verified_program(files, "DRBIOS.COM", DRBIOS_SHA256)?;
    let drshell = verified_program(files, "DRSHELL.COM", DRSHELL_SHA256)?;
    verify_drbios_lifetime(drbios)?;
    let shell_dispatch = analyze_shell_dispatch(drshell)?;

    let immediate_int18_ah1a_sequence_count = IMMEDIATE_GAIJI_SCAN_PROGRAMS
        .iter()
        .map(|name| {
            require_file(files, name)
                .map(|program| count_immediate_int18_ah1a_sequences(&program.bytes))
        })
        .sum::<Result<usize>>()?;
    ensure!(
        immediate_int18_ah1a_sequence_count == 0,
        "verified game programs gained an immediate INT 18h AH=1Ah sequence"
    );

    let full_window_raw_bitmap_bytes = EXTERNAL_CHARACTER_CANDIDATE_CAPACITY
        .checked_mul(CG_GLYPH_BYTES)
        .context("external-character bitmap population size overflow")?;

    Ok(LaunchChainAnalysis {
        launcher_scripts: vec![
            launcher_script("456.BAT", SHORT_LAUNCHER_SHA256),
            launcher_script("MADOU456.BAT", LONG_LAUNCHER_SHA256),
        ],
        resident_driver_lifetime: ResidentDriverLifetime {
            program: ProgramIdentity {
                name: "DRBIOS.COM",
                sha256: DRBIOS_SHA256,
            },
            entry_file_offset: DRBIOS_ENTRY_FILE_OFFSET,
            int64_install_file_offset: DRBIOS_INT64_INSTALL_FILE_OFFSET,
            terminate_and_stay_resident_file_offset: DRBIOS_TSR_FILE_OFFSET,
            remove_path_file_offset: DRBIOS_REMOVE_PATH_FILE_OFFSET,
            int64_restore_file_offset: DRBIOS_INT64_RESTORE_FILE_OFFSET,
            resident_block_release_file_offset: DRBIOS_RESIDENT_BLOCK_RELEASE_FILE_OFFSET,
            observation: "both launchers install DORI-BIOS before DRSHELL.COM and invoke its /R removal only after the shell returns; the exact driver installs INT 64h, terminates resident, then restores the vector and releases its resident block on removal",
        },
        shell_dispatch,
        external_character_installation_candidate: ExternalCharacterInstallationCandidate {
            status: "static placement candidate; not implemented or runtime verified",
            common_static_boundary: "after DRBIOS.COM installation and before DRSHELL.COM executes selector 0",
            text_consumer_programs: vec!["OPENING.COM", "MADO456.COM", "ENDING.COM"],
            candidate_glyph_capacity: EXTERNAL_CHARACTER_CANDIDATE_CAPACITY,
            observed_bytes_per_cg_glyph: CG_GLYPH_BYTES,
            full_window_raw_bitmap_bytes,
            drshell_zero_filled_file_tail_bytes: DRSHELL_FILE_SIZE
                - DRSHELL_EXEC_PARAMETER_BLOCK_FILE_OFFSET,
            full_window_fits_in_entire_zero_filled_file_tail: full_window_raw_bitmap_bytes
                <= DRSHELL_FILE_SIZE - DRSHELL_EXEC_PARAMETER_BLOCK_FILE_OFFSET,
            scanned_programs: IMMEDIATE_GAIJI_SCAN_PROGRAMS.to_vec(),
            immediate_int18_ah1a_sequence_count,
            observation: "a helper inserted at the pre-dispatch boundary, or a DRSHELL pre-dispatch loader, would run before all three verified text consumers; even the shell's entire zero-filled file tail is too small for 188 raw 32-byte glyphs, so a full window needs an external font asset or executable growth",
            limitation: "the immediate-sequence scan covers only MOV AH,1Ah/INT 18h and MOV AX,1Axxh/INT 18h byte forms; static launch order does not prove CG address bit-7 aliasing, BIOS registration visibility, persistence through every mode transition, or sufficient capacity for a translation",
        },
    })
}

fn launcher_script(name: &'static str, sha256: &'static str) -> LauncherScript {
    LauncherScript {
        name,
        sha256,
        normalized_command_sequence: vec![
            "FPLAY /b44",
            "DRBIOS install",
            "DRSHELL",
            "DRBIOS /r",
            "FPLAY",
        ],
    }
}

fn verify_launcher(
    files: &[GameFile],
    name: &str,
    expected_sha256: &str,
    expected_bytes: &[u8],
) -> Result<()> {
    let script = require_file(files, name)?;
    require_sha256(name, &script.bytes, expected_sha256)?;
    ensure!(
        script.bytes == expected_bytes,
        "{name} command sequence differs from the supported launcher"
    );
    Ok(())
}

fn verified_program<'a>(
    files: &'a [GameFile],
    name: &str,
    expected_sha256: &str,
) -> Result<&'a [u8]> {
    let file = require_file(files, name)?;
    require_sha256(name, &file.bytes, expected_sha256)?;
    Ok(&file.bytes)
}

fn verify_drbios_lifetime(drbios: &[u8]) -> Result<()> {
    require_bytes(drbios, 0x0000, &[0xe9, 0x8d, 0x23])?;
    require_bytes(
        drbios,
        DRBIOS_ENTRY_FILE_OFFSET,
        &[0xfc, 0x8c, 0xc8, 0x8e, 0xd8, 0xba, 0xaa, 0x27],
    )?;
    require_bytes(
        drbios,
        DRBIOS_TSR_FILE_OFFSET,
        &[0xb8, 0x00, 0x31, 0xcd, 0x21],
    )?;
    require_bytes(
        drbios,
        DRBIOS_REMOVE_PATH_FILE_OFFSET,
        &[0x2e, 0xa1, 0x62, 0x1f, 0x85, 0xc0],
    )?;
    require_bytes(
        drbios,
        DRBIOS_INT64_RESTORE_FILE_OFFSET,
        &[0xb8, 0x64, 0x25, 0xcd, 0x21],
    )?;
    require_bytes(
        drbios,
        DRBIOS_RESIDENT_BLOCK_RELEASE_FILE_OFFSET,
        &[0xb4, 0x49, 0xcd, 0x21],
    )?;
    require_bytes(
        drbios,
        DRBIOS_INT64_INSTALL_FILE_OFFSET,
        &[0xba, 0x03, 0x01, 0xb8, 0x64, 0x25, 0xcd, 0x21],
    )?;
    Ok(())
}

fn analyze_shell_dispatch(drshell: &[u8]) -> Result<ShellDispatch> {
    ensure!(
        drshell.len() == DRSHELL_FILE_SIZE,
        "DRSHELL.COM size changed: expected {DRSHELL_FILE_SIZE}, got {}",
        drshell.len()
    );
    require_bytes(
        drshell,
        DRSHELL_CHILD_LOOP_FILE_OFFSET,
        &[0x32, 0xc0, 0xe8, 0x09, 0x00, 0x3c, 0xff, 0x75, 0xf9],
    )?;
    require_bytes(
        drshell,
        DRSHELL_DISPATCH_FILE_OFFSET,
        &[0x8a, 0xd8, 0x8c, 0xc8, 0x8e, 0xd8, 0x8e, 0xc0],
    )?;
    require_bytes(
        drshell,
        DRSHELL_EXEC_FILE_OFFSET,
        &[0xb8, 0x00, 0x4b, 0xcd, 0x21],
    )?;
    require_bytes(
        drshell,
        DRSHELL_NEXT_SELECTOR_QUERY_FILE_OFFSET,
        &[0xb4, 0x03, 0xcd, 0x64],
    )?;
    require_bytes(
        drshell,
        DRSHELL_IMAGE_END_LITERAL_FILE_OFFSET - 1,
        &[0xbb, 0xe8, 0x04, 0xc1, 0xeb, 0x04, 0x83, 0xc3, 0x11],
    )?;
    let image_end_with_psp =
        usize::from(read_u16_le(drshell, DRSHELL_IMAGE_END_LITERAL_FILE_OFFSET)?);
    let allocation_paragraphs = (image_end_with_psp >> 4) + 0x11;

    let expected_selectors = [
        (0x0390, 0x03a4, "OPENING.COM"),
        (0x0395, 0x03b4, "MDSC.COM"),
        (0x039a, 0x03bf, "MADO456.COM"),
        (0x039f, 0x03cd, "ENDING.COM"),
        (0x039f, 0x03cd, "ENDING.COM"),
    ];
    let mut selectors = Vec::with_capacity(DRSHELL_SELECTOR_COUNT);
    for (selector, (expected_record, expected_filename, expected_name)) in
        expected_selectors.into_iter().enumerate()
    {
        let record_runtime = usize::from(read_u16_le(
            drshell,
            DRSHELL_SELECTOR_TABLE_FILE_OFFSET + selector * 2,
        )?);
        ensure!(
            record_runtime == expected_record,
            "DRSHELL.COM selector {selector} record changed"
        );
        let record_file_offset = runtime_to_file_offset(record_runtime)?;
        ensure!(
            drshell.get(record_file_offset) == Some(&1),
            "DRSHELL.COM selector {selector} no longer executes one child"
        );
        let filename_runtime = usize::from(read_u16_le(drshell, record_file_offset + 1)?);
        ensure!(
            filename_runtime == expected_filename,
            "DRSHELL.COM selector {selector} filename pointer changed"
        );
        let filename_file_offset = runtime_to_file_offset(filename_runtime)?;
        let program = read_ascii_c_string(drshell, filename_file_offset)?;
        ensure!(
            program.eq_ignore_ascii_case(expected_name),
            "DRSHELL.COM selector {selector} changed from {expected_name} to {program}"
        );
        selectors.push(ShellSelector {
            selector,
            record_file_offset,
            program: program.to_ascii_uppercase(),
            filename_file_offset,
        });
    }

    let zero_tail = drshell
        .get(DRSHELL_EXEC_PARAMETER_BLOCK_FILE_OFFSET..)
        .context("DRSHELL.COM zero-filled file tail is truncated")?;
    ensure!(
        zero_tail.iter().all(|byte| *byte == 0),
        "DRSHELL.COM zero-filled file tail changed"
    );

    Ok(ShellDispatch {
        program: ProgramIdentity {
            name: "DRSHELL.COM",
            sha256: DRSHELL_SHA256,
        },
        child_loop_file_offset: DRSHELL_CHILD_LOOP_FILE_OFFSET,
        dispatch_file_offset: DRSHELL_DISPATCH_FILE_OFFSET,
        child_exec_file_offset: DRSHELL_EXEC_FILE_OFFSET,
        next_selector_query_file_offset: DRSHELL_NEXT_SELECTOR_QUERY_FILE_OFFSET,
        selector_table_file_offset: DRSHELL_SELECTOR_TABLE_FILE_OFFSET,
        initial_selector: 0,
        selectors,
        allocation_paragraphs_before_child_exec: allocation_paragraphs,
        zero_filled_file_tail_offset: DRSHELL_EXEC_PARAMETER_BLOCK_FILE_OFFSET,
        zero_filled_file_tail_bytes: zero_tail.len(),
        exec_parameter_block_file_offset: DRSHELL_EXEC_PARAMETER_BLOCK_FILE_OFFSET,
        observation: "the shell starts with selector 0, executes one child through DOS AX=4B00h, and asks DORI-BIOS service AH=03h for the next selector; selectors 3 and 4 deliberately share ENDING.COM",
    })
}

fn runtime_to_file_offset(runtime_address: usize) -> Result<usize> {
    runtime_address
        .checked_sub(COM_LOAD_ORIGIN)
        .with_context(|| {
            format!("COM runtime address {runtime_address:#x} precedes its load origin")
        })
}

fn read_ascii_c_string(bytes: &[u8], file_offset: usize) -> Result<String> {
    let tail = bytes.get(file_offset..).with_context(|| {
        format!("string at file offset {file_offset:#x} is outside DRSHELL.COM")
    })?;
    let length = tail
        .iter()
        .position(|byte| *byte == 0)
        .with_context(|| format!("string at file offset {file_offset:#x} is not terminated"))?;
    let text = tail
        .get(..length)
        .context("DRSHELL.COM string range overflow")?;
    ensure!(
        text.is_ascii() && !text.is_empty(),
        "DRSHELL.COM string at file offset {file_offset:#x} is not nonempty ASCII"
    );
    Ok(String::from_utf8(text.to_vec()).expect("ASCII is valid UTF-8"))
}

fn count_immediate_int18_ah1a_sequences(bytes: &[u8]) -> usize {
    let mov_ah = bytes
        .windows(4)
        .filter(|window| *window == [0xb4, 0x1a, 0xcd, 0x18])
        .count();
    let mov_ax = bytes
        .windows(5)
        .filter(|window| window[0] == 0xb8 && window[2] == 0x1a && window[3..] == [0xcd, 0x18])
        .count();
    mov_ah + mov_ax
}

fn require_bytes(program: &[u8], file_offset: usize, expected: &[u8]) -> Result<()> {
    let actual = program
        .get(file_offset..file_offset + expected.len())
        .with_context(|| {
            format!("launch instruction at file offset {file_offset:#x} is truncated")
        })?;
    ensure!(
        actual == expected,
        "launch instruction changed at file offset {file_offset:#x}: expected {}, got {}",
        hex_bytes(expected),
        hex_bytes(actual)
    );
    Ok(())
}

#[cfg(test)]
#[path = "launch_chain_analysis_tests.rs"]
mod tests;
