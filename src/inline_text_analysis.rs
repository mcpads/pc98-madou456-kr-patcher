use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result, ensure};
use encoding_rs::SHIFT_JIS;
use serde::Serialize;

use crate::source_cd::GameFile;

use super::{
    COM_LOAD_ORIGIN, ENDING_SHA256, OPENING_SHA256, hex_bytes, read_u16_le, require_file,
    require_sha256,
};

const EXPECTED_INLINE_TEXT_ENTRY_COUNT: usize = 265;
const EXPECTED_INLINE_TEXT_SOURCE_BYTE_COUNT: usize = 6_178;
const EXPECTED_NON_CP932_INLINE_TEXT_ENTRY_COUNT: usize = 18;
const INLINE_TEXT_DESCRIPTOR_STRIDE: usize = 8;
const INLINE_TEXT_POINTER_FIELD_OFFSET: usize = 1;

const INLINE_TEXT_BLOCK_PROFILES: [InlineTextBlockProfile; 3] = [
    InlineTextBlockProfile {
        id: "opening_narration",
        role: "opening narration and character introductions",
        program: "OPENING.COM",
        program_sha256: OPENING_SHA256,
        descriptor_table_file_offset: 0x5092,
        entry_count: 33,
        consumer_file_offset: 0x18bc,
    },
    InlineTextBlockProfile {
        id: "ending_scenarios",
        role: "team-specific ending dialogue and narration",
        program: "ENDING.COM",
        program_sha256: ENDING_SHA256,
        descriptor_table_file_offset: 0x52c6,
        entry_count: 188,
        consumer_file_offset: 0x2f3b,
    },
    InlineTextBlockProfile {
        id: "staff_credits",
        role: "ending staff credits",
        program: "ENDING.COM",
        program_sha256: ENDING_SHA256,
        descriptor_table_file_offset: 0x6d30,
        entry_count: 44,
        consumer_file_offset: 0x2f9d,
    },
];

const INLINE_TEXT_ARENA_PROFILES: [InlineTextArenaProfile; 4] = [
    InlineTextArenaProfile {
        id: "opening_narration_pool",
        block_id: "opening_narration",
        program: "OPENING.COM",
        first_entry: 0,
        entry_count: 33,
        file_start: 0x519a,
        file_end: 0x54fa,
        source_leading_zero_count: 1,
        source_separator_zero_count: 1,
    },
    InlineTextArenaProfile {
        id: "ending_scenarios_pool",
        block_id: "ending_scenarios",
        program: "ENDING.COM",
        first_entry: 0,
        entry_count: 188,
        file_start: 0x58a6,
        file_end: 0x6d30,
        source_leading_zero_count: 1,
        source_separator_zero_count: 1,
    },
    InlineTextArenaProfile {
        id: "staff_credits_first_pool",
        block_id: "staff_credits",
        program: "ENDING.COM",
        first_entry: 0,
        entry_count: 40,
        file_start: 0x6e90,
        file_end: 0x70a3,
        source_leading_zero_count: 1,
        source_separator_zero_count: 1,
    },
    InlineTextArenaProfile {
        id: "staff_credits_final_pool",
        block_id: "staff_credits",
        program: "ENDING.COM",
        first_entry: 40,
        entry_count: 4,
        file_start: 0x70c6,
        file_end: 0x7100,
        source_leading_zero_count: 1,
        source_separator_zero_count: 1,
    },
];

const INLINE_TEXT_PROTECTED_SPANS: [InlineTextProtectedSpanProfile; 1] = [
    InlineTextProtectedSpanProfile {
        id: "unreferenced_staff_literal",
        program: "ENDING.COM",
        file_start: 0x70a3,
        file_end: 0x70c6,
        sha256: "034a68088f1a0abb6c5f71c5be4e088657f084c99fb739ff5206519b5c73cf60",
        status: "not referenced by the verified 44-entry staff descriptor table; preserve unchanged until a consumer is proven",
    },
];

#[derive(Clone, Copy)]
pub(crate) struct InlineTextBlockProfile {
    pub(crate) id: &'static str,
    pub(crate) role: &'static str,
    pub(crate) program: &'static str,
    pub(crate) program_sha256: &'static str,
    pub(crate) descriptor_table_file_offset: usize,
    pub(crate) entry_count: usize,
    pub(crate) consumer_file_offset: usize,
}

#[derive(Clone, Copy)]
pub(crate) struct InlineTextArenaProfile {
    pub(crate) id: &'static str,
    pub(crate) block_id: &'static str,
    pub(crate) program: &'static str,
    pub(crate) first_entry: usize,
    pub(crate) entry_count: usize,
    pub(crate) file_start: usize,
    pub(crate) file_end: usize,
    pub(crate) source_leading_zero_count: usize,
    pub(crate) source_separator_zero_count: usize,
}

#[derive(Clone, Copy)]
struct InlineTextProtectedSpanProfile {
    id: &'static str,
    program: &'static str,
    file_start: usize,
    file_end: usize,
    sha256: &'static str,
    status: &'static str,
}

#[derive(Debug, Serialize)]
pub(crate) struct InlineTextAnalysisReport {
    pub(crate) block_count: usize,
    pub(crate) entry_count: usize,
    source_byte_count: usize,
    strict_cp932_entry_count: usize,
    non_cp932_entry_count: usize,
    blocks: Vec<InlineTextBlockSummary>,
    source_layout_program_roundtrips: Vec<InlineTextProgramRoundtrip>,
    single_nul_relocation_source_capacity: usize,
    single_nul_relocation_source_slack: usize,
    storage_arenas: Vec<InlineTextStorageArenaSummary>,
    protected_spans: Vec<InlineTextProtectedSpanSummary>,
}

#[derive(Debug, Serialize)]
struct InlineTextProgramRoundtrip {
    program: &'static str,
    program_sha256: &'static str,
    source_layout_rebuild_identical: bool,
}

#[derive(Debug, Serialize)]
struct InlineTextStorageArenaSummary {
    id: &'static str,
    block_id: &'static str,
    program: &'static str,
    first_entry: usize,
    entry_count: usize,
    file_start: usize,
    file_end: usize,
    byte_capacity: usize,
    current_source_byte_count: usize,
    source_layout_required_bytes: usize,
    source_layout_slack: usize,
    single_nul_relocation_source_capacity: usize,
    single_nul_relocation_source_slack: usize,
}

#[derive(Debug, Serialize)]
struct InlineTextProtectedSpanSummary {
    id: &'static str,
    program: &'static str,
    file_start: usize,
    file_end: usize,
    size: usize,
    sha256: &'static str,
    status: &'static str,
}

struct InlineTextStorageAnalysis {
    source_layout_program_roundtrips: Vec<InlineTextProgramRoundtrip>,
    single_nul_relocation_source_capacity: usize,
    single_nul_relocation_source_slack: usize,
    storage_arenas: Vec<InlineTextStorageArenaSummary>,
    protected_spans: Vec<InlineTextProtectedSpanSummary>,
}

#[derive(Debug, Serialize)]
pub(crate) struct InlineTextBlockSummary {
    id: &'static str,
    role: &'static str,
    program: &'static str,
    program_sha256: &'static str,
    descriptor_table_file_offset: usize,
    descriptor_table_runtime_address: usize,
    descriptor_stride: usize,
    pointer_field_offset: usize,
    pub(crate) entry_count: usize,
    pub(crate) source_byte_count: usize,
    pub(crate) strict_cp932_entry_count: usize,
    non_cp932_entry_count: usize,
    consumer_file_offset: usize,
    consumer_runtime_address: usize,
}

#[derive(Debug, Serialize)]
pub(crate) struct InlineTextCatalog {
    pub(crate) analysis: InlineTextAnalysisReport,
    blocks: Vec<InlineTextBlockCatalog>,
}

impl InlineTextCatalog {
    pub(crate) fn translation_layouts(&self) -> impl Iterator<Item = (&str, usize)> {
        self.blocks
            .iter()
            .flat_map(|block| block.entries.iter().map(|entry| (entry.id.as_str(), 1)))
    }

    pub(crate) fn translation_source_cell_counts(
        &self,
    ) -> std::collections::BTreeMap<String, Vec<usize>> {
        self.blocks
            .iter()
            .flat_map(|block| &block.entries)
            .map(|entry| {
                (
                    entry.id.clone(),
                    vec![entry.decoded_text_lossy.chars().count()],
                )
            })
            .collect()
    }

    pub(crate) fn translation_font_source_codes(&self) -> BTreeSet<[u8; 2]> {
        let mut codes = BTreeSet::new();
        for entry in self.blocks.iter().flat_map(|block| &block.entries) {
            for pair in entry.source.windows(2) {
                let code = [pair[0], pair[1]];
                if crate::translation_font::is_translation_font_code(code) {
                    codes.insert(code);
                }
            }
        }
        codes
    }

    pub(crate) fn rebuild_with_translations(
        &self,
        files: &[GameFile],
        program_bases: &BTreeMap<String, Vec<u8>>,
        translations: &BTreeMap<&str, &[String]>,
        codebook: &BTreeMap<char, [u8; 2]>,
    ) -> Result<Vec<RebuiltInlineProgram>> {
        ensure!(
            translations.len() == self.analysis.entry_count,
            "inline translation population changed before reinsertion"
        );
        let mut rebuilt_programs = Vec::with_capacity(2);
        for (program_name, program_sha256) in [
            ("OPENING.COM", OPENING_SHA256),
            ("ENDING.COM", ENDING_SHA256),
        ] {
            let source_program = require_file(files, program_name)?;
            require_sha256(program_name, &source_program.bytes, program_sha256)?;
            let base = program_bases
                .get(program_name)
                .with_context(|| format!("inline reinsertion lacks base {program_name}"))?;
            ensure!(
                base.len() >= source_program.bytes.len(),
                "inline reinsertion base {program_name} is shorter than its immutable source"
            );
            let pool_file_offset = base.len();
            let mut pointer_writes = Vec::new();
            let mut planned_records = Vec::new();
            let mut pool = Vec::new();
            let mut changed_entry_count = 0usize;
            let mut used_external_characters = BTreeSet::new();

            for profile in INLINE_TEXT_BLOCK_PROFILES
                .iter()
                .copied()
                .filter(|profile| profile.program == program_name)
            {
                let block = require_inline_text_block(&self.blocks, profile.id)?;
                for entry in &block.entries {
                    let segments = translations.get(entry.id.as_str()).with_context(|| {
                        format!("translation draft lacks inline entry {}", entry.id)
                    })?;
                    ensure!(
                        segments.len() == 1,
                        "inline translation entry {} changed its one-segment layout",
                        entry.id
                    );
                    let encoded = if segments[0].is_empty() {
                        entry.source.clone()
                    } else {
                        encode_two_byte_renderer_text(
                            &segments[0],
                            codebook,
                            &mut used_external_characters,
                        )?
                    };
                    ensure!(
                        !encoded.is_empty() && !encoded.contains(&0),
                        "inline translation entry {} encoded to an empty or NUL-containing value",
                        entry.id
                    );
                    changed_entry_count += usize::from(encoded != entry.source);
                    let text_file_offset = pool_file_offset
                        .checked_add(pool.len())
                        .context("inline translated pool offset overflow")?;
                    let text_runtime_address = text_file_offset
                        .checked_add(COM_LOAD_ORIGIN)
                        .context("inline translated pointer overflow")?;
                    let text_runtime_address = u16::try_from(text_runtime_address)
                        .context("inline translated pointer exceeds the COM segment")?;
                    let pointer_offset = entry
                        .descriptor_file_offset
                        .checked_add(INLINE_TEXT_POINTER_FIELD_OFFSET)
                        .context("inline descriptor pointer offset overflow")?;
                    pointer_writes.push(crate::expected_write::FixedRangeExpectedWrite {
                        owner: "inline-translation-pool",
                        purpose: "relocate a descriptor-owned string into the appended pool",
                        offset: pointer_offset,
                        expected_source: (entry.text_runtime_address as u16).to_le_bytes().to_vec(),
                        replacement: text_runtime_address.to_le_bytes().to_vec(),
                    });
                    planned_records.push((pointer_offset, text_file_offset, encoded.clone()));
                    append_translated_inline_string(&mut pool, &encoded);
                }
            }

            let mut updated =
                crate::expected_write::apply_fixed_range_expected_writes(base, &pointer_writes)?;
            updated.extend_from_slice(&pool);
            ensure!(
                updated.len() + COM_LOAD_ORIGIN <= 0x1_0000,
                "translated {program_name} exceeds the COM segment"
            );
            ensure!(
                updated.get(pool_file_offset..) == Some(pool.as_slice()),
                "translated {program_name} pool readback differs"
            );
            for (pointer_offset, text_file_offset, encoded) in &planned_records {
                let pointer = read_u16_le(&updated, *pointer_offset)? as usize;
                ensure!(
                    pointer == text_file_offset + COM_LOAD_ORIGIN,
                    "translated {program_name} descriptor pointer readback differs"
                );
                let text_end = text_file_offset + encoded.len();
                ensure!(
                    updated.get(*text_file_offset..text_end) == Some(encoded.as_slice())
                        && updated.get(text_end..text_end + 2) == Some(&[0, 0]),
                    "translated {program_name} string readback differs"
                );
            }
            rebuilt_programs.push(RebuiltInlineProgram {
                name: program_name.to_owned(),
                bytes: updated,
                changed_entry_count,
                pointer_expected_write_count: pointer_writes.len(),
                pool_file_offset,
                pool_size: pool.len(),
                used_external_characters,
            });
        }
        Ok(rebuilt_programs)
    }
}

fn append_translated_inline_string(pool: &mut Vec<u8>, encoded: &[u8]) {
    pool.extend_from_slice(encoded);
    pool.extend_from_slice(&[0, 0]);
}

pub(crate) struct RebuiltInlineProgram {
    pub(crate) name: String,
    pub(crate) bytes: Vec<u8>,
    pub(crate) changed_entry_count: usize,
    pub(crate) pointer_expected_write_count: usize,
    pub(crate) pool_file_offset: usize,
    pub(crate) pool_size: usize,
    pub(crate) used_external_characters: BTreeSet<char>,
}

pub(crate) fn encode_two_byte_renderer_text(
    text: &str,
    codebook: &BTreeMap<char, [u8; 2]>,
    used_external_characters: &mut BTreeSet<char>,
) -> Result<Vec<u8>> {
    ensure!(
        !text.is_empty(),
        "cannot encode empty inline translation text"
    );
    let runtime_characters = crate::josa::runtime_text_characters(text)?;
    let mut encoded = Vec::with_capacity(runtime_characters.len() * 2);
    for character in runtime_characters {
        ensure!(
            !character.is_control(),
            "inline translation contains control character U+{:04X}",
            u32::from(character)
        );
        if let Some(code) = codebook.get(&character) {
            encoded.extend_from_slice(code);
            used_external_characters.insert(character);
            continue;
        }
        let full_width = match character {
            ' ' => '\u{3000}',
            '\u{21}'..='\u{7e}' => char::from_u32(u32::from(character) + 0xfee0)
                .expect("ASCII full-width projection is valid Unicode"),
            _ => character,
        };
        let value = full_width.to_string();
        let (bytes, _, had_errors) = SHIFT_JIS.encode(&value);
        ensure!(
            !had_errors && bytes.len() == 2,
            "inline translation character {character:?} is not one two-byte renderer cell"
        );
        ensure!(
            !crate::translation_font::contains_translation_font_code(bytes.as_ref()),
            "inline CP932 character {character:?} collides with the translation font sheet"
        );
        encoded.extend_from_slice(bytes.as_ref());
    }
    Ok(encoded)
}

#[derive(Debug, Serialize)]
pub(crate) struct InlineTextBlockCatalog {
    pub(crate) id: &'static str,
    pub(crate) entries: Vec<InlineTextEntry>,
}

#[derive(Debug, Serialize)]
pub(crate) struct InlineTextEntry {
    pub(crate) id: String,
    pub(crate) descriptor_index: usize,
    pub(crate) descriptor_file_offset: usize,
    pub(crate) text_file_offset: usize,
    pub(crate) text_runtime_address: usize,
    #[serde(skip)]
    pub(crate) source: Vec<u8>,
    pub(crate) source_hex: String,
    pub(crate) decoded_text_lossy: String,
    pub(crate) strict_cp932: bool,
}

pub(crate) fn catalog_inline_text(files: &[GameFile]) -> Result<InlineTextCatalog> {
    let mut blocks = Vec::with_capacity(INLINE_TEXT_BLOCK_PROFILES.len());
    let mut summaries = Vec::with_capacity(INLINE_TEXT_BLOCK_PROFILES.len());

    for profile in INLINE_TEXT_BLOCK_PROFILES {
        let program = require_file(files, profile.program)?;
        require_sha256(profile.program, &program.bytes, profile.program_sha256)?;
        let (summary, entries) = parse_inline_text_block(&program.bytes, profile)?;
        summaries.push(summary);
        blocks.push(InlineTextBlockCatalog {
            id: profile.id,
            entries,
        });
    }

    let entry_count = summaries.iter().map(|block| block.entry_count).sum();
    ensure!(
        entry_count == EXPECTED_INLINE_TEXT_ENTRY_COUNT,
        "inline text entry population changed: expected {EXPECTED_INLINE_TEXT_ENTRY_COUNT}, got {entry_count}"
    );
    let source_byte_count = summaries.iter().map(|block| block.source_byte_count).sum();
    let strict_cp932_entry_count = summaries
        .iter()
        .map(|block| block.strict_cp932_entry_count)
        .sum();
    let non_cp932_entry_count = summaries
        .iter()
        .map(|block| block.non_cp932_entry_count)
        .sum();
    ensure!(
        source_byte_count == EXPECTED_INLINE_TEXT_SOURCE_BYTE_COUNT,
        "inline text source-byte population changed: expected {EXPECTED_INLINE_TEXT_SOURCE_BYTE_COUNT}, got {source_byte_count}"
    );
    ensure!(
        non_cp932_entry_count == EXPECTED_NON_CP932_INLINE_TEXT_ENTRY_COUNT,
        "non-CP932 inline text population changed: expected {EXPECTED_NON_CP932_INLINE_TEXT_ENTRY_COUNT}, got {non_cp932_entry_count}"
    );
    let storage = analyze_inline_text_storage(files, &blocks)?;

    Ok(InlineTextCatalog {
        analysis: InlineTextAnalysisReport {
            block_count: summaries.len(),
            entry_count,
            source_byte_count,
            strict_cp932_entry_count,
            non_cp932_entry_count,
            blocks: summaries,
            source_layout_program_roundtrips: storage.source_layout_program_roundtrips,
            single_nul_relocation_source_capacity: storage.single_nul_relocation_source_capacity,
            single_nul_relocation_source_slack: storage.single_nul_relocation_source_slack,
            storage_arenas: storage.storage_arenas,
            protected_spans: storage.protected_spans,
        },
        blocks,
    })
}

fn analyze_inline_text_storage(
    files: &[GameFile],
    blocks: &[InlineTextBlockCatalog],
) -> Result<InlineTextStorageAnalysis> {
    let mut storage_arenas = Vec::with_capacity(INLINE_TEXT_ARENA_PROFILES.len());
    let mut single_nul_relocation_source_capacity = 0usize;
    let mut single_nul_relocation_source_slack = 0usize;
    for profile in INLINE_TEXT_ARENA_PROFILES {
        let block = require_inline_text_block(blocks, profile.block_id)?;
        let entry_end = profile
            .first_entry
            .checked_add(profile.entry_count)
            .context("inline text arena entry range overflow")?;
        let entries = block
            .entries
            .get(profile.first_entry..entry_end)
            .with_context(|| {
                format!(
                    "inline text arena {} entry range is outside block {}",
                    profile.id, profile.block_id
                )
            })?;
        let byte_capacity = profile
            .file_end
            .checked_sub(profile.file_start)
            .context("inline text arena has descending file bounds")?;
        let current_source_byte_count: usize = entries.iter().map(|entry| entry.source.len()).sum();
        let source_layout_required_bytes = profile
            .source_leading_zero_count
            .checked_add(current_source_byte_count)
            .and_then(|size| size.checked_add(entries.len()))
            .and_then(|size| {
                size.checked_add(
                    profile.source_separator_zero_count * entries.len().saturating_sub(1),
                )
            })
            .context("inline text source layout size overflow")?;
        ensure!(
            source_layout_required_bytes <= byte_capacity,
            "inline text arena {} source layout exceeds its byte capacity",
            profile.id
        );
        let arena_single_nul_source_capacity = byte_capacity
            .checked_sub(entries.len())
            .context("inline text arena cannot hold one NUL per entry")?;
        ensure!(
            current_source_byte_count <= arena_single_nul_source_capacity,
            "inline text arena {} cannot hold its current sources in a single-NUL layout",
            profile.id
        );
        let arena_single_nul_source_slack =
            arena_single_nul_source_capacity - current_source_byte_count;
        single_nul_relocation_source_capacity += arena_single_nul_source_capacity;
        single_nul_relocation_source_slack += arena_single_nul_source_slack;
        storage_arenas.push(InlineTextStorageArenaSummary {
            id: profile.id,
            block_id: profile.block_id,
            program: profile.program,
            first_entry: profile.first_entry,
            entry_count: profile.entry_count,
            file_start: profile.file_start,
            file_end: profile.file_end,
            byte_capacity,
            current_source_byte_count,
            source_layout_required_bytes,
            source_layout_slack: byte_capacity - source_layout_required_bytes,
            single_nul_relocation_source_capacity: arena_single_nul_source_capacity,
            single_nul_relocation_source_slack: arena_single_nul_source_slack,
        });
    }

    let mut protected_spans = Vec::with_capacity(INLINE_TEXT_PROTECTED_SPANS.len());
    for profile in INLINE_TEXT_PROTECTED_SPANS {
        let program = require_file(files, profile.program)?;
        let bytes = program
            .bytes
            .get(profile.file_start..profile.file_end)
            .with_context(|| format!("protected inline span {} is truncated", profile.id))?;
        require_sha256(profile.id, bytes, profile.sha256)?;
        protected_spans.push(InlineTextProtectedSpanSummary {
            id: profile.id,
            program: profile.program,
            file_start: profile.file_start,
            file_end: profile.file_end,
            size: bytes.len(),
            sha256: profile.sha256,
            status: profile.status,
        });
    }

    let mut source_layout_program_roundtrips = Vec::with_capacity(2);
    for (program_name, program_sha256) in [
        ("OPENING.COM", OPENING_SHA256),
        ("ENDING.COM", ENDING_SHA256),
    ] {
        let program = require_file(files, program_name)?;
        let rebuilt = rebuild_inline_text_source_layout(program, blocks)?;
        let source_layout_rebuild_identical = rebuilt == program.bytes;
        ensure!(
            source_layout_rebuild_identical,
            "{program_name} inline text source-layout rebuild changed program bytes"
        );
        source_layout_program_roundtrips.push(InlineTextProgramRoundtrip {
            program: program_name,
            program_sha256,
            source_layout_rebuild_identical,
        });
    }

    Ok(InlineTextStorageAnalysis {
        source_layout_program_roundtrips,
        single_nul_relocation_source_capacity,
        single_nul_relocation_source_slack,
        storage_arenas,
        protected_spans,
    })
}

fn rebuild_inline_text_source_layout(
    program: &GameFile,
    blocks: &[InlineTextBlockCatalog],
) -> Result<Vec<u8>> {
    let mut rebuilt = program.bytes.clone();
    for profile in INLINE_TEXT_ARENA_PROFILES
        .iter()
        .copied()
        .filter(|profile| profile.program == program.display_name)
    {
        let block = require_inline_text_block(blocks, profile.block_id)?;
        write_inline_text_arena(&mut rebuilt, block, profile)?;
    }
    Ok(rebuilt)
}

fn require_inline_text_block<'a>(
    blocks: &'a [InlineTextBlockCatalog],
    block_id: &str,
) -> Result<&'a InlineTextBlockCatalog> {
    let mut matching = blocks.iter().filter(|block| block.id == block_id);
    let block = matching
        .next()
        .with_context(|| format!("inline text block {block_id} is missing"))?;
    ensure!(
        matching.next().is_none(),
        "inline text block {block_id} is duplicated"
    );
    Ok(block)
}

pub(crate) fn write_inline_text_arena(
    program: &mut [u8],
    block: &InlineTextBlockCatalog,
    profile: InlineTextArenaProfile,
) -> Result<()> {
    ensure!(
        block.id == profile.block_id,
        "inline text arena {} received block {} instead of {}",
        profile.id,
        block.id,
        profile.block_id
    );
    let entry_end = profile
        .first_entry
        .checked_add(profile.entry_count)
        .context("inline text arena entry range overflow")?;
    let entries = block
        .entries
        .get(profile.first_entry..entry_end)
        .with_context(|| {
            format!(
                "inline text arena {} entry range is outside block {}",
                profile.id, profile.block_id
            )
        })?;
    let arena = program
        .get_mut(profile.file_start..profile.file_end)
        .with_context(|| format!("inline text arena {} is truncated", profile.id))?;
    arena.fill(0);
    let mut cursor = profile
        .file_start
        .checked_add(profile.source_leading_zero_count)
        .context("inline text arena cursor overflow")?;

    for (arena_index, entry) in entries.iter().enumerate() {
        let source_end = cursor
            .checked_add(entry.source.len())
            .context("inline text source end overflow")?;
        let terminator_end = source_end
            .checked_add(1)
            .context("inline text terminator end overflow")?;
        ensure!(
            terminator_end <= profile.file_end,
            "inline text arena {} overflows while writing entry {}",
            profile.id,
            entry.id
        );
        let runtime_address = cursor
            .checked_add(COM_LOAD_ORIGIN)
            .context("inline text runtime address overflow")?;
        let runtime_address = u16::try_from(runtime_address)
            .context("inline text runtime address does not fit a COM pointer")?;
        let pointer_offset = entry
            .descriptor_file_offset
            .checked_add(INLINE_TEXT_POINTER_FIELD_OFFSET)
            .context("inline text descriptor pointer offset overflow")?;
        let pointer = program
            .get_mut(pointer_offset..pointer_offset + 2)
            .with_context(|| format!("inline text descriptor {} is truncated", entry.id))?;
        pointer.copy_from_slice(&runtime_address.to_le_bytes());
        program[cursor..source_end].copy_from_slice(&entry.source);
        cursor = terminator_end;
        if arena_index + 1 < entries.len() {
            cursor = cursor
                .checked_add(profile.source_separator_zero_count)
                .context("inline text separator end overflow")?;
            ensure!(
                cursor <= profile.file_end,
                "inline text arena {} overflows after entry {}",
                profile.id,
                entry.id
            );
        }
    }
    Ok(())
}

pub(crate) fn parse_inline_text_block(
    program: &[u8],
    profile: InlineTextBlockProfile,
) -> Result<(InlineTextBlockSummary, Vec<InlineTextEntry>)> {
    let descriptor_bytes = profile
        .entry_count
        .checked_mul(INLINE_TEXT_DESCRIPTOR_STRIDE)
        .context("inline text descriptor table size overflow")?;
    let descriptor_end = profile
        .descriptor_table_file_offset
        .checked_add(descriptor_bytes)
        .context("inline text descriptor table offset overflow")?;
    ensure!(
        descriptor_end <= program.len(),
        "{} {} descriptor table is truncated",
        profile.program,
        profile.id
    );

    let mut entries = Vec::with_capacity(profile.entry_count);
    let mut previous_text_end = descriptor_end;
    let mut source_byte_count = 0usize;
    let mut strict_cp932_entry_count = 0usize;
    for descriptor_index in 0..profile.entry_count {
        let descriptor_file_offset =
            profile.descriptor_table_file_offset + descriptor_index * INLINE_TEXT_DESCRIPTOR_STRIDE;
        let pointer_offset = descriptor_file_offset + INLINE_TEXT_POINTER_FIELD_OFFSET;
        let text_runtime_address = read_u16_le(program, pointer_offset)? as usize;
        let text_file_offset = text_runtime_address
            .checked_sub(COM_LOAD_ORIGIN)
            .context("inline text pointer is below the COM load origin")?;
        ensure!(
            text_file_offset >= previous_text_end && text_file_offset < program.len(),
            "{} {} entry {descriptor_index} text offsets are not strictly increasing",
            profile.program,
            profile.id
        );
        let terminator = program[text_file_offset..]
            .iter()
            .position(|byte| *byte == 0)
            .map(|relative| text_file_offset + relative)
            .with_context(|| {
                format!(
                    "{} {} entry {descriptor_index} has no NUL terminator",
                    profile.program, profile.id
                )
            })?;
        ensure!(
            terminator > text_file_offset,
            "{} {} entry {descriptor_index} is empty",
            profile.program,
            profile.id
        );
        let source = &program[text_file_offset..terminator];
        let strict_cp932 = SHIFT_JIS
            .decode_without_bom_handling_and_without_replacement(source)
            .is_some();
        strict_cp932_entry_count += usize::from(strict_cp932);
        let (decoded_text_lossy, _) = SHIFT_JIS.decode_without_bom_handling(source);
        source_byte_count += source.len();
        entries.push(InlineTextEntry {
            id: format!("{}:{descriptor_index:03}", profile.id),
            descriptor_index,
            descriptor_file_offset,
            text_file_offset,
            text_runtime_address,
            source: source.to_vec(),
            source_hex: hex_bytes(source),
            decoded_text_lossy: decoded_text_lossy.into_owned(),
            strict_cp932,
        });
        previous_text_end = terminator + 1;
    }

    Ok((
        InlineTextBlockSummary {
            id: profile.id,
            role: profile.role,
            program: profile.program,
            program_sha256: profile.program_sha256,
            descriptor_table_file_offset: profile.descriptor_table_file_offset,
            descriptor_table_runtime_address: profile.descriptor_table_file_offset
                + COM_LOAD_ORIGIN,
            descriptor_stride: INLINE_TEXT_DESCRIPTOR_STRIDE,
            pointer_field_offset: INLINE_TEXT_POINTER_FIELD_OFFSET,
            entry_count: entries.len(),
            source_byte_count,
            strict_cp932_entry_count,
            non_cp932_entry_count: entries.len() - strict_cp932_entry_count,
            consumer_file_offset: profile.consumer_file_offset,
            consumer_runtime_address: profile.consumer_file_offset + COM_LOAD_ORIGIN,
        },
        entries,
    ))
}

#[cfg(test)]
#[path = "inline_text_analysis_tests.rs"]
mod tests;
