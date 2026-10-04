use anyhow::{Context, Result, ensure};
use serde::Serialize;

use crate::source_cd::GameFile;

use super::{
    DRBIOS_SHA256, ENDING_SHA256, MADO456_SHA256, OPENING_SHA256, hex_bytes, require_file,
    require_sha256,
};

const BIOS_GAIJI_FIRST_JIS: u16 = 0x7621;
const BIOS_GAIJI_LAST_JIS: u16 = 0x777e;
const BIOS_GAIJI_FIRST_SHIFT_JIS: [u8; 2] = [0xeb, 0x9f];
const BIOS_GAIJI_LAST_SHIFT_JIS: [u8; 2] = [0xec, 0x9e];
pub(super) const EXTERNAL_CHARACTER_CANDIDATE_CAPACITY: usize = 188;
pub(super) const CG_GLYPH_BYTES: usize = 32;

#[derive(Debug, Serialize)]
pub(super) struct TextRendererAnalysis {
    message_path: MessageRendererPath,
    inline_paths: Vec<InlineRendererPath>,
    external_character_candidate: ExternalCharacterCandidate,
}

#[derive(Debug, Serialize)]
struct MessageRendererPath {
    source_program: ProgramIdentity,
    message_consumer_file_offset: usize,
    direct_pair_file_offset: usize,
    code_buffer_write_file_offset: usize,
    code_buffer_renderer_file_offset: usize,
    glyph_service_interrupt: &'static str,
    glyph_service_program: ProgramIdentity,
    glyph_service_dispatch_file_offset: usize,
    glyph_service_handler_file_offset: usize,
    shift_jis_to_cg_file_offset: usize,
    cg_port_read_file_offset: usize,
    observation: &'static str,
}

#[derive(Debug, Serialize)]
struct InlineRendererPath {
    program: ProgramIdentity,
    descriptor_consumer_file_offset: usize,
    shift_jis_to_cg_file_offset: usize,
    cg_port_read_file_offset: usize,
    observation: &'static str,
}

#[derive(Debug, Serialize)]
struct ProgramIdentity {
    name: &'static str,
    sha256: &'static str,
}

#[derive(Debug, Serialize)]
struct ExternalCharacterCandidate {
    status: &'static str,
    candidate_jis_first: String,
    candidate_jis_last: String,
    candidate_shift_jis_first: String,
    candidate_shift_jis_last: String,
    candidate_capacity: usize,
    first_code_addresses: CandidateCgAddresses,
    last_code_addresses: CandidateCgAddresses,
    observation: &'static str,
    limitation: &'static str,
}

#[derive(Debug, Serialize)]
struct CandidateCgAddresses {
    shift_jis: String,
    drbios_a1_a3: String,
    opening_ending_a1_a3: String,
    same_a1_and_a3_low_7_bits: bool,
    a3_bit_7_differs: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct CgPortAddress {
    a1: u8,
    a3: u8,
}

pub(super) fn analyze_text_renderers(files: &[GameFile]) -> Result<TextRendererAnalysis> {
    let main = verified_program(files, "MADO456.COM", MADO456_SHA256)?;
    let drbios = verified_program(files, "DRBIOS.COM", DRBIOS_SHA256)?;
    let opening = verified_program(files, "OPENING.COM", OPENING_SHA256)?;
    let ending = verified_program(files, "ENDING.COM", ENDING_SHA256)?;

    require_bytes(main, 0x0766, &[0x8a, 0xd0, 0x8a, 0x34, 0x46])?;
    require_bytes(main, 0x077e, &[0xe8, 0x01, 0x7b, 0x26, 0x89, 0x15])?;
    require_bytes(main, 0x1fd3, &[0x8b, 0x1c])?;
    require_bytes(main, 0x1fe9, &[0xe8, 0xa0, 0x07])?;
    require_bytes(
        main,
        0x2798,
        &[0x86, 0xf2, 0xb4, 0x16, 0xcd, 0x64, 0x8b, 0xf2],
    )?;

    require_bytes(drbios, 0x0065, &[0x73, 0x07])?;
    require_bytes(drbios, 0x0673, &[0xe8, 0x16, 0x00])?;
    require_bytes(
        drbios,
        0x068c,
        &[
            0xd0, 0xe6, 0x80, 0xea, 0x1f, 0x78, 0x06, 0x80, 0xfa, 0x61, 0x80, 0xd2, 0xde, 0x81,
            0xc2, 0xa1, 0x1f, 0x81, 0xe2, 0x7f, 0x7f, 0x80, 0xee, 0x20,
        ],
    )?;
    require_bytes(
        drbios,
        0x06a4,
        &[
            0xb0, 0x0b, 0xe6, 0x68, 0x92, 0xe6, 0xa1, 0x86, 0xe0, 0xe6, 0xa3,
        ],
    )?;
    require_bytes(
        drbios,
        0x06d3,
        &[
            0x20, 0xe6, 0xa5, 0xe4, 0xa9, 0xaa, 0x8b, 0xc1, 0xe6, 0xa5, 0xe4, 0xa9, 0xaa,
        ],
    )?;

    let inline_conversion = [
        0x86, 0xc4, 0x80, 0xfc, 0xe0, 0x72, 0x00, 0xd0, 0xe4, 0x3c, 0x80, 0x14, 0xe1, 0x79, 0x04,
        0x2c, 0x5e, 0xfe, 0xc4, 0x2d, 0x01, 0xe1, 0xab, 0x49, 0x75, 0xe1, 0xc3,
    ];
    let inline_cg_read = [
        0xe6, 0xa1, 0x8a, 0xc4, 0xe6, 0xa3, 0xb5, 0x10, 0x32, 0xff, 0x8a, 0xc7, 0xe6, 0xa5, 0xe4,
        0xa9,
    ];
    require_bytes(opening, 0x259e, &inline_conversion)?;
    require_bytes(opening, 0x260f, &inline_cg_read)?;
    require_bytes(ending, 0x3a6f, &inline_conversion)?;
    require_bytes(ending, 0x3ae0, &inline_cg_read)?;
    require_bytes(ending, 0x3089, &inline_cg_read)?;

    Ok(TextRendererAnalysis {
        message_path: MessageRendererPath {
            source_program: ProgramIdentity {
                name: "MADO456.COM",
                sha256: MADO456_SHA256,
            },
            message_consumer_file_offset: 0x0723,
            direct_pair_file_offset: 0x0766,
            code_buffer_write_file_offset: 0x0781,
            code_buffer_renderer_file_offset: 0x278c,
            glyph_service_interrupt: "INT 64h AH=16h",
            glyph_service_program: ProgramIdentity {
                name: "DRBIOS.COM",
                sha256: DRBIOS_SHA256,
            },
            glyph_service_dispatch_file_offset: 0x0065,
            glyph_service_handler_file_offset: 0x0673,
            shift_jis_to_cg_file_offset: 0x068c,
            cg_port_read_file_offset: 0x06a9,
            observation: "the message consumer stores each mapped or direct Shift-JIS pair in the game code buffer; the renderer swaps it into DX, requests the resident DORI-BIOS glyph service, and consumes its returned 32-byte CG glyph",
        },
        inline_paths: vec![
            InlineRendererPath {
                program: ProgramIdentity {
                    name: "OPENING.COM",
                    sha256: OPENING_SHA256,
                },
                descriptor_consumer_file_offset: 0x18bc,
                shift_jis_to_cg_file_offset: 0x2583,
                cg_port_read_file_offset: 0x260f,
                observation: "converts each descriptor-owned Shift-JIS pair to a CG address, reads 16 two-byte rows through ports A1/A3/A5/A9, and draws the resulting glyph into graphics VRAM",
            },
            InlineRendererPath {
                program: ProgramIdentity {
                    name: "ENDING.COM",
                    sha256: ENDING_SHA256,
                },
                descriptor_consumer_file_offset: 0x2f3b,
                shift_jis_to_cg_file_offset: 0x3a54,
                cg_port_read_file_offset: 0x3ae0,
                observation: "the ending dialogue converts Shift-JIS through the shared converter, then uses the normal-size CG glyph reader and VRAM writer",
            },
            InlineRendererPath {
                program: ProgramIdentity {
                    name: "ENDING.COM",
                    sha256: ENDING_SHA256,
                },
                descriptor_consumer_file_offset: 0x2f9d,
                shift_jis_to_cg_file_offset: 0x3a54,
                cg_port_read_file_offset: 0x3089,
                observation: "staff credits use a separate CG glyph loop; its row-expansion table doubles pixels horizontally and its VRAM writer doubles rows while revealing the animated text",
            },
        ],
        external_character_candidate: external_character_candidate()?,
    })
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

fn require_bytes(program: &[u8], file_offset: usize, expected: &[u8]) -> Result<()> {
    let actual = program
        .get(file_offset..file_offset + expected.len())
        .with_context(|| {
            format!("renderer instruction at file offset {file_offset:#x} is truncated")
        })?;
    ensure!(
        actual == expected,
        "renderer instruction changed at file offset {file_offset:#x}: expected {}, got {}",
        hex_bytes(expected),
        hex_bytes(actual)
    );
    Ok(())
}

fn external_character_candidate() -> Result<ExternalCharacterCandidate> {
    Ok(ExternalCharacterCandidate {
        status: "static-compatible candidate; not runtime verified",
        candidate_jis_first: format!("{BIOS_GAIJI_FIRST_JIS:04x}"),
        candidate_jis_last: format!("{BIOS_GAIJI_LAST_JIS:04x}"),
        candidate_shift_jis_first: hex_bytes(&BIOS_GAIJI_FIRST_SHIFT_JIS),
        candidate_shift_jis_last: hex_bytes(&BIOS_GAIJI_LAST_SHIFT_JIS),
        candidate_capacity: EXTERNAL_CHARACTER_CANDIDATE_CAPACITY,
        first_code_addresses: candidate_addresses(BIOS_GAIJI_FIRST_SHIFT_JIS)?,
        last_code_addresses: candidate_addresses(BIOS_GAIJI_LAST_SHIFT_JIS)?,
        observation: "both renderer families accept the candidate two-byte codes and select the same A1 cell plus the same low seven A3 row bits",
        limitation: "OPENING.COM and ENDING.COM retain A3 bit 7 where DRBIOS.COM clears it; this analysis does not prove that the hardware aliases those addresses, that INT 18h AH=1Ah registration reaches every path, that glyph state survives program transitions, or that 188 cells cover a translation",
    })
}

fn candidate_addresses(shift_jis: [u8; 2]) -> Result<CandidateCgAddresses> {
    let drbios = drbios_cg_address(shift_jis)?;
    let inline = inline_cg_address(shift_jis)?;
    Ok(CandidateCgAddresses {
        shift_jis: hex_bytes(&shift_jis),
        drbios_a1_a3: port_pair(drbios),
        opening_ending_a1_a3: port_pair(inline),
        same_a1_and_a3_low_7_bits: drbios.a1 == inline.a1 && drbios.a3 & 0x7f == inline.a3 & 0x7f,
        a3_bit_7_differs: drbios.a3 ^ inline.a3 == 0x80,
    })
}

fn drbios_cg_address([lead, trail]: [u8; 2]) -> Result<CgPortAddress> {
    require_shift_jis_pair(lead, trail)?;
    let mut row = lead.wrapping_shl(1);
    let mut cell = trail.wrapping_sub(0x1f);
    if cell & 0x80 == 0 {
        cell = cell.wrapping_add(0xde).wrapping_add(u8::from(cell < 0x61));
    }
    let address = ((u16::from(row) << 8) | u16::from(cell)).wrapping_add(0x1fa1) & 0x7f7f;
    row = (address >> 8) as u8;
    Ok(CgPortAddress {
        a1: address as u8,
        a3: row.wrapping_sub(0x20),
    })
}

fn inline_cg_address([lead, trail]: [u8; 2]) -> Result<CgPortAddress> {
    require_shift_jis_pair(lead, trail)?;
    let mut high = lead.wrapping_shl(1);
    let mut low = trail
        .wrapping_add(0xe1)
        .wrapping_add(u8::from(trail < 0x80));
    if low & 0x80 != 0 {
        low = low.wrapping_sub(0x5e);
        high = high.wrapping_add(1);
    }
    let address = ((u16::from(high) << 8) | u16::from(low)).wrapping_sub(0xe101);
    Ok(CgPortAddress {
        a1: address as u8,
        a3: (address >> 8) as u8 - 0x20,
    })
}

fn require_shift_jis_pair(lead: u8, trail: u8) -> Result<()> {
    ensure!(
        matches!(lead, 0x81..=0x9f | 0xe0..=0xef) && matches!(trail, 0x40..=0x7e | 0x80..=0xfc),
        "invalid renderer Shift-JIS pair {lead:02x}{trail:02x}"
    );
    Ok(())
}

fn port_pair(address: CgPortAddress) -> String {
    format!("{:02x}/{:02x}", address.a1, address.a3)
}

#[cfg(test)]
#[path = "text_renderer_analysis_tests.rs"]
mod tests;
