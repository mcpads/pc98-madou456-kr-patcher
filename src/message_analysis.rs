use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result, ensure};
use encoding_rs::SHIFT_JIS;
use serde::Serialize;

use crate::compile_lz::{decode_exact_compile_lz, encode_compile_lz};
use crate::source_cd::GameFile;

use super::{MADO456_SHA256, hex_bytes, require_file, require_sha256, sha256_hex};

pub(crate) const FIRST_GLYPH_TABLE_FILE_OFFSET: usize = 0xe848;
pub(crate) const SECOND_GLYPH_TABLE_FILE_OFFSET: usize = 0xe908;
pub(crate) const FIRST_GLYPH_TABLE_ENTRIES: usize = 0x60;
pub(crate) const SECOND_GLYPH_TABLE_ENTRIES: usize = 0x40;
const MESSAGE_OFFSET_SLOT_COUNT: usize = 256;
const MESSAGE_OFFSET_TABLE_SIZE: usize = MESSAGE_OFFSET_SLOT_COUNT * 2;
const MESSAGE_DELIMITER: u8 = 0xff;
const EXPECTED_MESSAGE_ENTRY_COUNT: usize = 1_356;
const EXPECTED_TEXT_SEGMENT_COUNT: usize = 2_610;
const EXPECTED_MULTI_SEGMENT_ENTRY_COUNT: usize = 788;
const EXPECTED_MAX_TEXT_SEGMENTS_PER_ENTRY: usize = 20;
const EXPECTED_ENTRIES_WITH_NON_LINE_CONTROL_BETWEEN_TEXT: usize = 216;
const EXPECTED_CONTROL_OPCODE_COUNTS: [(&str, usize); 5] = [
    ("00", 1_572),
    ("01", 1_518),
    ("02", 1_100),
    ("03", 1_298),
    ("05", 1_498),
];

const MESSAGE_FILE_NAMES: [&str; 15] = [
    "MSG.DAT",
    "MSGEV.DAT",
    "MSG01.DAT",
    "MSG02.DAT",
    "MSG03.DAT",
    "MSG04.DAT",
    "MSG05.DAT",
    "MSG06.DAT",
    "MSG07.DAT",
    "MSG08.DAT",
    "MSG09.DAT",
    "MSG10.DAT",
    "MSG11.DAT",
    "MSG12.DAT",
    "MSG13.DAT",
];

#[derive(Debug, Serialize)]
pub(crate) struct MessageFormatReport {
    offset_slot_count: usize,
    offset_table_size: usize,
    record_delimiter_hex: String,
    first_single_byte_range: String,
    first_mapping_table_file_offset: usize,
    second_single_byte_range: String,
    second_mapping_table_file_offset: usize,
    direct_cp932_lead_ranges: Vec<String>,
    control_argument_opcodes_hex: Vec<String>,
}

#[derive(Debug, Serialize)]
#[cfg(feature = "analysis")]
pub(crate) struct MessageFileSummary {
    pub(crate) name: String,
    pub(crate) packed_size: usize,
    pub(crate) decoded_size: usize,
    pub(crate) populated_entries: usize,
    pub(crate) decoded_serialization_identical: bool,
    pub(crate) repacked_size: usize,
    pub(crate) repacked_sha256: String,
    pub(crate) repacked_self_roundtrip: bool,
    pub(crate) source_packed_reproduced: bool,
}

#[derive(Debug, Serialize)]
pub(crate) struct MessageCatalog {
    source_program: SourceProgram,
    pub(crate) format: MessageFormatReport,
    pub(crate) message_file_count: usize,
    pub(crate) message_entry_count: usize,
    pub(crate) files: Vec<MessageFileCatalog>,
    #[serde(skip)]
    #[cfg_attr(not(feature = "analysis"), allow(dead_code))]
    pub(crate) control_layout: MessageControlLayoutReport,
    #[serde(skip)]
    #[cfg_attr(not(feature = "analysis"), allow(dead_code))]
    pub(crate) empty_segment_reassembly_identical: bool,
    #[serde(skip)]
    text_encoding: MessageTextEncoding,
}

impl MessageCatalog {
    pub(crate) fn translation_layouts(&self) -> impl Iterator<Item = (&str, usize)> {
        self.files.iter().flat_map(|file| {
            file.entries
                .iter()
                .map(|entry| (entry.id.as_str(), text_token_count(&entry.tokens)))
        })
    }

    pub(crate) fn translation_source_cell_counts(&self) -> BTreeMap<String, Vec<usize>> {
        self.files
            .iter()
            .flat_map(|file| &file.entries)
            .map(|entry| {
                let counts = entry
                    .tokens
                    .iter()
                    .filter_map(|token| match token {
                        MessageToken::Text { text, .. } => Some(text.chars().count()),
                        MessageToken::Control { .. } => None,
                    })
                    .collect();
                (entry.id.clone(), counts)
            })
            .collect()
    }

    #[cfg(feature = "analysis")]
    pub(crate) fn translation_entries(&self) -> impl Iterator<Item = (&str, &[MessageToken])> {
        self.files.iter().flat_map(|file| {
            file.entries
                .iter()
                .map(|entry| (entry.id.as_str(), entry.tokens.as_slice()))
        })
    }

    pub(crate) fn translation_font_source_codes(&self) -> Result<BTreeSet<[u8; 2]>> {
        let mut codes = self.text_encoding.translation_font_source_codes.clone();
        for token in self
            .files
            .iter()
            .flat_map(|file| &file.entries)
            .flat_map(|entry| &entry.tokens)
        {
            let MessageToken::Text { cp932_hex, .. } = token else {
                continue;
            };
            let bytes = decode_hex(cp932_hex)?;
            for pair in bytes.windows(2) {
                let code = [pair[0], pair[1]];
                if crate::translation_font::is_translation_font_code(code) {
                    codes.insert(code);
                }
            }
        }
        Ok(codes)
    }

    pub(crate) fn rebuild_with_translations(
        &self,
        translations: &BTreeMap<&str, &[String]>,
        external_characters: &BTreeMap<char, [u8; 2]>,
    ) -> Result<Vec<RebuiltMessageFile>> {
        ensure!(
            translations.len() == self.message_entry_count,
            "message translation population changed before reinsertion"
        );
        let mut rebuilt_files = Vec::with_capacity(self.files.len());
        for file in &self.files {
            let mut records = Vec::with_capacity(file.entries.len());
            let mut changed_entry_count = 0usize;
            let mut used_external_characters = BTreeSet::new();
            for entry in &file.entries {
                let segments = translations.get(entry.id.as_str()).with_context(|| {
                    format!("translation draft lacks message entry {}", entry.id)
                })?;
                let source = decode_hex(&entry.source_hex)?;
                let reassembled = reassemble_message_tokens(&entry.tokens, segments, |text| {
                    self.text_encoding.encode(
                        text,
                        external_characters,
                        &mut used_external_characters,
                    )
                })?;
                changed_entry_count += usize::from(reassembled != source);
                records.push(MessageRecordBytes {
                    slot: entry.slot,
                    source: reassembled,
                });
            }
            let decoded = serialize_message_records(&records)?;
            let packed = encode_compile_lz(&decoded);
            let roundtrip = decode_exact_compile_lz(&packed)
                .with_context(|| format!("rebuilt {} is not exact Compile-LZ", file.name))?;
            ensure!(
                roundtrip.streams.len() == 1 && roundtrip.streams[0] == decoded,
                "rebuilt {} does not restore its serialized messages",
                file.name
            );
            let source_packed_reproduced = sha256_hex(&packed) == file.packed_sha256;
            ensure!(
                changed_entry_count > 0 || source_packed_reproduced,
                "unchanged {} does not reproduce its source packed bytes",
                file.name
            );
            rebuilt_files.push(RebuiltMessageFile {
                name: file.name.clone(),
                decoded,
                packed,
                changed_entry_count,
                source_packed_reproduced,
                used_external_characters,
            });
        }
        Ok(rebuilt_files)
    }
}

#[derive(Debug)]
pub(crate) struct RebuiltMessageFile {
    pub(crate) name: String,
    #[cfg_attr(not(feature = "analysis"), allow(dead_code))]
    pub(crate) decoded: Vec<u8>,
    pub(crate) packed: Vec<u8>,
    pub(crate) changed_entry_count: usize,
    #[cfg_attr(not(feature = "analysis"), allow(dead_code))]
    pub(crate) source_packed_reproduced: bool,
    pub(crate) used_external_characters: BTreeSet<char>,
}

#[derive(Debug)]
struct MessageTextEncoding {
    single_byte_by_character: BTreeMap<char, u8>,
    translation_font_source_codes: BTreeSet<[u8; 2]>,
}

impl MessageTextEncoding {
    fn from_mapping_tables(first_mapping: &[[u8; 2]], second_mapping: &[[u8; 2]]) -> Result<Self> {
        let mut single_byte_by_character = BTreeMap::new();
        let translation_font_source_codes = first_mapping
            .iter()
            .chain(second_mapping)
            .copied()
            .filter(|code| crate::translation_font::is_translation_font_code(*code))
            .collect();
        for (source_byte, encoded) in (0x20_u8..=0x7f).zip(first_mapping.iter()) {
            add_single_byte_mapping(&mut single_byte_by_character, source_byte, encoded)?;
        }
        for (source_byte, encoded) in (0xa0_u8..=0xdf).zip(second_mapping.iter()) {
            add_single_byte_mapping(&mut single_byte_by_character, source_byte, encoded)?;
        }
        Ok(Self {
            single_byte_by_character,
            translation_font_source_codes,
        })
    }

    fn encode(
        &self,
        text: &str,
        external_characters: &BTreeMap<char, [u8; 2]>,
        used_external_characters: &mut BTreeSet<char>,
    ) -> Result<Vec<u8>> {
        ensure!(
            !text.is_empty(),
            "cannot encode an empty translation segment"
        );
        let mut encoded = Vec::new();
        for character in crate::josa::runtime_text_characters(text)? {
            ensure!(
                !character.is_control(),
                "translation text contains control character U+{:04X}",
                u32::from(character)
            );
            if let Some(external) = external_characters.get(&character) {
                encoded.extend_from_slice(external);
                used_external_characters.insert(character);
                continue;
            }

            let mapped_character = match character {
                ' ' => '\u{3000}',
                '\u{21}'..='\u{7e}' => char::from_u32(u32::from(character) + 0xfee0)
                    .expect("ASCII full-width projection is valid Unicode"),
                _ => character,
            };
            if let Some(source_byte) = self.single_byte_by_character.get(&mapped_character) {
                encoded.push(*source_byte);
                continue;
            }

            let text = mapped_character.to_string();
            let (direct, _, had_errors) = SHIFT_JIS.encode(&text);
            ensure!(
                !had_errors && direct.len() == 2,
                "translation character {character:?} has no source-table, direct CP932-pair, or external-codebook encoding"
            );
            validate_replacement_message_text(&direct)?;
            encoded.extend_from_slice(&direct);
        }
        validate_replacement_message_text(&encoded)?;
        Ok(encoded)
    }
}

fn add_single_byte_mapping(
    mappings: &mut BTreeMap<char, u8>,
    source_byte: u8,
    encoded: &[u8; 2],
) -> Result<()> {
    let text = SHIFT_JIS
        .decode_without_bom_handling_and_without_replacement(encoded)
        .context("message single-byte mapping is not valid CP932")?;
    let mut characters = text.chars();
    let character = characters
        .next()
        .context("message single-byte mapping decoded to no character")?;
    ensure!(
        characters.next().is_none(),
        "message single-byte mapping decoded to more than one character"
    );
    mappings.entry(character).or_insert(source_byte);
    Ok(())
}

#[derive(Debug, Serialize)]
struct SourceProgram {
    name: &'static str,
    sha256: &'static str,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct MessageControlLayoutReport {
    entry_count: usize,
    text_segment_count: usize,
    multi_segment_entry_count: usize,
    max_text_segments_per_entry: usize,
    entries_with_non_line_control_between_text: usize,
    control_opcode_counts: Vec<ControlOpcodeCount>,
    implication: &'static str,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
struct ControlOpcodeCount {
    opcode_hex: String,
    count: usize,
}

#[derive(Debug, Serialize)]
pub(crate) struct MessageFileCatalog {
    pub(crate) name: String,
    pub(crate) packed_size: usize,
    packed_sha256: String,
    pub(crate) decoded_size: usize,
    decoded_sha256: String,
    offset_slot_count: usize,
    pub(crate) populated_entries: usize,
    pub(crate) decoded_serialization_identical: bool,
    pub(crate) repacked_size: usize,
    pub(crate) repacked_sha256: String,
    pub(crate) repacked_self_roundtrip: bool,
    pub(crate) source_packed_reproduced: bool,
    entries: Vec<MessageEntry>,
}

pub(crate) struct MessageRecordBytes {
    pub(crate) slot: usize,
    pub(crate) source: Vec<u8>,
}

#[derive(Debug, Serialize)]
struct MessageEntry {
    id: String,
    slot: usize,
    decoded_offset: usize,
    source_hex: String,
    decoded_text: String,
    tokens: Vec<MessageToken>,
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum MessageToken {
    Text {
        source_hex: String,
        cp932_hex: String,
        text: String,
    },
    Control {
        opcode_hex: String,
        argument_hex: Option<String>,
    },
}

pub(crate) fn catalog_messages(files: &[GameFile]) -> Result<MessageCatalog> {
    let main_program = require_file(files, "MADO456.COM")?;
    require_sha256("MADO456.COM", &main_program.bytes, MADO456_SHA256)?;
    let first_mapping = read_mapping_table(
        &main_program.bytes,
        FIRST_GLYPH_TABLE_FILE_OFFSET,
        FIRST_GLYPH_TABLE_ENTRIES,
    )?;
    let second_mapping = read_mapping_table(
        &main_program.bytes,
        SECOND_GLYPH_TABLE_FILE_OFFSET,
        SECOND_GLYPH_TABLE_ENTRIES,
    )?;
    let text_encoding = MessageTextEncoding::from_mapping_tables(&first_mapping, &second_mapping)?;

    let mut message_files = Vec::with_capacity(MESSAGE_FILE_NAMES.len());
    for name in MESSAGE_FILE_NAMES {
        let file = require_file(files, name)?;
        message_files.push(parse_message_file(file, &first_mapping, &second_mapping)?);
    }
    let message_entry_count: usize = message_files.iter().map(|file| file.entries.len()).sum();
    ensure!(
        message_entry_count == EXPECTED_MESSAGE_ENTRY_COUNT,
        "message entry population changed: expected {EXPECTED_MESSAGE_ENTRY_COUNT}, got {message_entry_count}"
    );
    let control_layout = summarize_message_control_layout(
        message_files
            .iter()
            .flat_map(|file| file.entries.iter().map(|entry| entry.tokens.as_slice())),
    );
    ensure!(
        control_layout.text_segment_count == EXPECTED_TEXT_SEGMENT_COUNT
            && control_layout.multi_segment_entry_count == EXPECTED_MULTI_SEGMENT_ENTRY_COUNT
            && control_layout.max_text_segments_per_entry == EXPECTED_MAX_TEXT_SEGMENTS_PER_ENTRY
            && control_layout.entries_with_non_line_control_between_text
                == EXPECTED_ENTRIES_WITH_NON_LINE_CONTROL_BETWEEN_TEXT,
        "message text/control layout population changed"
    );
    let expected_control_opcode_counts = EXPECTED_CONTROL_OPCODE_COUNTS
        .iter()
        .map(|(opcode_hex, count)| ControlOpcodeCount {
            opcode_hex: (*opcode_hex).to_owned(),
            count: *count,
        })
        .collect::<Vec<_>>();
    ensure!(
        control_layout.control_opcode_counts == expected_control_opcode_counts,
        "message control opcode population changed"
    );

    Ok(MessageCatalog {
        source_program: SourceProgram {
            name: "MADO456.COM",
            sha256: MADO456_SHA256,
        },
        format: message_format_report(),
        message_file_count: message_files.len(),
        message_entry_count,
        files: message_files,
        control_layout,
        empty_segment_reassembly_identical: true,
        text_encoding,
    })
}

fn summarize_message_control_layout<'a>(
    entries: impl IntoIterator<Item = &'a [MessageToken]>,
) -> MessageControlLayoutReport {
    let mut entry_count = 0usize;
    let mut text_segment_count = 0usize;
    let mut multi_segment_entry_count = 0usize;
    let mut max_text_segments_per_entry = 0usize;
    let mut entries_with_non_line_control_between_text = 0usize;
    let mut control_opcode_counts = BTreeMap::<String, usize>::new();

    for tokens in entries {
        entry_count += 1;
        let entry_text_segment_count = tokens
            .iter()
            .filter(|token| matches!(token, MessageToken::Text { .. }))
            .count();
        text_segment_count += entry_text_segment_count;
        multi_segment_entry_count += usize::from(entry_text_segment_count > 1);
        max_text_segments_per_entry = max_text_segments_per_entry.max(entry_text_segment_count);

        let has_non_line_control_between_text = tokens.iter().enumerate().any(|(index, token)| {
            matches!(token, MessageToken::Control { opcode_hex, .. } if opcode_hex != "02")
                && tokens[..index]
                    .iter()
                    .any(|token| matches!(token, MessageToken::Text { .. }))
                && tokens[index + 1..]
                    .iter()
                    .any(|token| matches!(token, MessageToken::Text { .. }))
        });
        entries_with_non_line_control_between_text +=
            usize::from(has_non_line_control_between_text);

        for token in tokens {
            if let MessageToken::Control { opcode_hex, .. } = token {
                *control_opcode_counts.entry(opcode_hex.clone()).or_default() += 1;
            }
        }
    }

    MessageControlLayoutReport {
        entry_count,
        text_segment_count,
        multi_segment_entry_count,
        max_text_segments_per_entry,
        entries_with_non_line_control_between_text,
        control_opcode_counts: control_opcode_counts
            .into_iter()
            .map(|(opcode_hex, count)| ControlOpcodeCount { opcode_hex, count })
            .collect(),
        implication: "translation text must preserve source-owned control-token positions instead of flattening each message into one editable string",
    }
}

fn parse_message_file(
    file: &GameFile,
    first_mapping: &[[u8; 2]],
    second_mapping: &[[u8; 2]],
) -> Result<MessageFileCatalog> {
    let mut decoded = decode_exact_compile_lz(&file.bytes)
        .with_context(|| format!("{} is not exact Compile-LZ", file.display_name))?;
    ensure!(
        decoded.streams.len() == 1,
        "{} has more than one Compile-LZ stream",
        file.display_name
    );
    let bytes = decoded.streams.remove(0);
    ensure!(
        bytes.len() >= MESSAGE_OFFSET_TABLE_SIZE,
        "{} decoded data is shorter than its message offset table",
        file.display_name
    );
    let offsets: Vec<usize> = bytes[..MESSAGE_OFFSET_TABLE_SIZE]
        .as_chunks::<2>()
        .0
        .iter()
        .map(|word| usize::from(u16::from_le_bytes([word[0], word[1]])))
        .collect();
    let populated: Vec<(usize, usize)> = offsets
        .iter()
        .copied()
        .enumerate()
        .filter(|(_, offset)| *offset != 0)
        .collect();
    ensure!(
        !populated.is_empty(),
        "{} has no populated message offsets",
        file.display_name
    );

    let mut entries = Vec::with_capacity(populated.len());
    let mut records = Vec::with_capacity(populated.len());
    for (position, (slot, offset)) in populated.iter().copied().enumerate() {
        ensure!(
            offset >= MESSAGE_OFFSET_TABLE_SIZE && offset < bytes.len(),
            "{} slot {slot:#04x} has out-of-range offset {offset:#06x}",
            file.display_name
        );
        ensure!(
            bytes[offset - 1] == MESSAGE_DELIMITER,
            "{} slot {slot:#04x} offset is not preceded by 0xff",
            file.display_name
        );
        let delimiter = bytes[offset..]
            .iter()
            .position(|byte| *byte == MESSAGE_DELIMITER)
            .map(|relative| offset + relative)
            .with_context(|| {
                format!(
                    "{} slot {slot:#04x} lacks a 0xff delimiter",
                    file.display_name
                )
            })?;
        if let Some((_, next_offset)) = populated.get(position + 1) {
            ensure!(
                delimiter + 1 == *next_offset,
                "{} has unindexed bytes between slots {slot:#04x} and {:#04x}",
                file.display_name,
                populated[position + 1].0
            );
        } else {
            ensure!(
                delimiter + 1 == bytes.len(),
                "{} has trailing bytes after its final message delimiter",
                file.display_name
            );
        }
        let source = &bytes[offset..delimiter];
        records.push(MessageRecordBytes {
            slot,
            source: source.to_vec(),
        });
        let (decoded_text, tokens) =
            decode_message_tokens(source, first_mapping, second_mapping)
                .with_context(|| format!("decode {} slot {slot:#04x}", file.display_name))?;
        let empty_segments = vec![String::new(); text_token_count(&tokens)];
        let reassembled = reassemble_message_tokens(&tokens, &empty_segments, |_| {
            anyhow::bail!("empty translation segment unexpectedly requested encoding")
        })?;
        ensure!(
            reassembled == source,
            "{} slot {slot:#04x} changed during empty-segment token reassembly",
            file.display_name
        );
        entries.push(MessageEntry {
            id: format!("{}:{slot:02X}", file.display_name),
            slot,
            decoded_offset: offset,
            source_hex: hex_bytes(source),
            decoded_text,
            tokens,
        });
    }

    let serialized = serialize_message_records(&records)?;
    let decoded_serialization_identical = serialized == bytes;
    ensure!(
        decoded_serialization_identical,
        "{} decoded messages do not survive unchanged serialization",
        file.display_name
    );
    let repacked = encode_compile_lz(&serialized);
    let repacked_decoded = decode_exact_compile_lz(&repacked)
        .with_context(|| format!("repacked {} is not exact Compile-LZ", file.display_name))?;
    let repacked_self_roundtrip = repacked_decoded.streams == [serialized];
    ensure!(
        repacked_self_roundtrip,
        "repacked {} does not restore its serialized messages",
        file.display_name
    );
    let source_packed_reproduced = repacked == file.bytes;
    ensure!(
        source_packed_reproduced,
        "{} unchanged messages do not reproduce their source packed bytes",
        file.display_name
    );

    Ok(MessageFileCatalog {
        name: file.display_name.clone(),
        packed_size: file.bytes.len(),
        packed_sha256: sha256_hex(&file.bytes),
        decoded_size: bytes.len(),
        decoded_sha256: sha256_hex(&bytes),
        offset_slot_count: MESSAGE_OFFSET_SLOT_COUNT,
        populated_entries: entries.len(),
        decoded_serialization_identical,
        repacked_size: repacked.len(),
        repacked_sha256: sha256_hex(&repacked),
        repacked_self_roundtrip,
        source_packed_reproduced,
        entries,
    })
}

pub(crate) fn reassemble_message_tokens(
    tokens: &[MessageToken],
    translation_segments: &[String],
    mut encode_translation: impl FnMut(&str) -> Result<Vec<u8>>,
) -> Result<Vec<u8>> {
    let expected_segment_count = text_token_count(tokens);
    ensure!(
        translation_segments.len() == expected_segment_count,
        "message translation has {} segments but source owns {expected_segment_count}",
        translation_segments.len()
    );

    let mut segment_index = 0usize;
    let mut reassembled = Vec::new();
    for token in tokens {
        match token {
            MessageToken::Text { source_hex, .. } => {
                let translation = &translation_segments[segment_index];
                segment_index += 1;
                if translation.is_empty() {
                    reassembled.extend_from_slice(&decode_hex(source_hex)?);
                } else {
                    let lines = crate::josa::manual_message_lines(translation)?;
                    for (line_index, line) in lines.iter().enumerate() {
                        if line_index > 0 {
                            reassembled.push(0x02);
                        }
                        let encoded = encode_translation(line)?;
                        validate_replacement_message_text(&encoded)?;
                        reassembled.extend_from_slice(&encoded);
                    }
                }
            }
            MessageToken::Control {
                opcode_hex,
                argument_hex,
            } => {
                let opcode = decode_single_hex_byte(opcode_hex, "message control opcode")?;
                ensure!(
                    opcode <= 0x1f,
                    "message control opcode 0x{opcode:02x} is outside the control range"
                );
                let takes_argument = matches!(opcode, 0x00 | 0x01 | 0x04 | 0x05 | 0x06 | 0x07);
                ensure!(
                    argument_hex.is_some() == takes_argument,
                    "message control opcode 0x{opcode:02x} has the wrong argument arity"
                );
                reassembled.push(opcode);
                if let Some(argument_hex) = argument_hex {
                    reassembled.push(decode_single_hex_byte(
                        argument_hex,
                        "message control argument",
                    )?);
                }
            }
        }
    }
    ensure!(
        segment_index == translation_segments.len(),
        "message translation segment population changed during reassembly"
    );
    Ok(reassembled)
}

fn text_token_count(tokens: &[MessageToken]) -> usize {
    tokens
        .iter()
        .filter(|token| matches!(token, MessageToken::Text { .. }))
        .count()
}

fn validate_replacement_message_text(bytes: &[u8]) -> Result<()> {
    ensure!(!bytes.is_empty(), "encoded message text is empty");
    let mut cursor = 0usize;
    while cursor < bytes.len() {
        let byte = bytes[cursor];
        cursor += 1;
        if (0x20..=0x7f).contains(&byte) || (0xa0..=0xdf).contains(&byte) {
            continue;
        }
        ensure!(
            (0x80..=0x9f).contains(&byte) || (0xe0..=0xfe).contains(&byte),
            "encoded message text contains reserved byte 0x{byte:02x}"
        );
        let trail = *bytes.get(cursor).with_context(|| {
            format!("encoded message lead byte 0x{byte:02x} lacks a trail byte")
        })?;
        ensure!(
            (0x40..=0x7e).contains(&trail) || (0x80..=0xfc).contains(&trail),
            "encoded message text has invalid trail byte 0x{trail:02x}"
        );
        cursor += 1;
    }
    Ok(())
}

fn decode_single_hex_byte(value: &str, label: &str) -> Result<u8> {
    let decoded = decode_hex(value).with_context(|| format!("decode {label}"))?;
    ensure!(decoded.len() == 1, "{label} must contain exactly one byte");
    Ok(decoded[0])
}

fn decode_hex(value: &str) -> Result<Vec<u8>> {
    let (pairs, remainder) = value.as_bytes().as_chunks::<2>();
    ensure!(
        remainder.is_empty(),
        "hex value has an odd number of digits"
    );
    let mut decoded = Vec::with_capacity(pairs.len());
    for pair in pairs {
        let high = decode_hex_nibble(pair[0]).context("invalid high hex digit")?;
        let low = decode_hex_nibble(pair[1]).context("invalid low hex digit")?;
        decoded.push((high << 4) | low);
    }
    Ok(decoded)
}

fn decode_hex_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

pub(crate) fn serialize_message_records(records: &[MessageRecordBytes]) -> Result<Vec<u8>> {
    ensure!(
        !records.is_empty(),
        "cannot serialize an empty message table"
    );
    let mut bytes = vec![0; MESSAGE_OFFSET_TABLE_SIZE];
    bytes.push(MESSAGE_DELIMITER);
    let mut previous_slot = None;

    for record in records {
        ensure!(
            record.slot < MESSAGE_OFFSET_SLOT_COUNT,
            "message slot {:#x} is outside the offset table",
            record.slot
        );
        if let Some(previous) = previous_slot {
            ensure!(
                record.slot > previous,
                "message slots must be unique and strictly increasing"
            );
        }
        let offset = u16::try_from(bytes.len()).context("message data exceeds 16-bit offsets")?;
        bytes[record.slot * 2..record.slot * 2 + 2].copy_from_slice(&offset.to_le_bytes());
        bytes.extend_from_slice(&record.source);
        bytes.push(MESSAGE_DELIMITER);
        previous_slot = Some(record.slot);
    }
    Ok(bytes)
}

pub(crate) fn decode_message_tokens(
    source: &[u8],
    first_mapping: &[[u8; 2]],
    second_mapping: &[[u8; 2]],
) -> Result<(String, Vec<MessageToken>)> {
    let mut cursor = 0usize;
    let mut tokens = Vec::new();
    let mut decoded_text = String::new();
    let mut source_text_bytes = Vec::new();
    let mut cp932_bytes = Vec::new();

    while cursor < source.len() {
        let byte = source[cursor];
        cursor += 1;
        if byte <= 0x1f {
            flush_text_token(
                &mut tokens,
                &mut decoded_text,
                &mut source_text_bytes,
                &mut cp932_bytes,
            )?;
            let argument = if matches!(byte, 0x00 | 0x01 | 0x04 | 0x05 | 0x06 | 0x07) {
                let argument = *source
                    .get(cursor)
                    .with_context(|| format!("control opcode 0x{byte:02x} lacks its argument"))?;
                cursor += 1;
                Some(argument)
            } else {
                None
            };
            if byte == 0x02 {
                decoded_text.push('\n');
            }
            tokens.push(MessageToken::Control {
                opcode_hex: format!("{byte:02x}"),
                argument_hex: argument.map(|argument| format!("{argument:02x}")),
            });
            continue;
        }

        source_text_bytes.push(byte);
        if (0x20..=0x7f).contains(&byte) {
            cp932_bytes.extend_from_slice(&first_mapping[usize::from(byte - 0x20)]);
        } else if (0xa0..=0xdf).contains(&byte) {
            cp932_bytes.extend_from_slice(&second_mapping[usize::from(byte - 0xa0)]);
        } else {
            let trail = *source
                .get(cursor)
                .with_context(|| format!("CP932 lead byte 0x{byte:02x} lacks a trail byte"))?;
            cursor += 1;
            source_text_bytes.push(trail);
            cp932_bytes.extend_from_slice(&[byte, trail]);
        }
    }
    flush_text_token(
        &mut tokens,
        &mut decoded_text,
        &mut source_text_bytes,
        &mut cp932_bytes,
    )?;
    Ok((decoded_text, tokens))
}

fn flush_text_token(
    tokens: &mut Vec<MessageToken>,
    decoded_text: &mut String,
    source_bytes: &mut Vec<u8>,
    cp932_bytes: &mut Vec<u8>,
) -> Result<()> {
    if source_bytes.is_empty() {
        ensure!(cp932_bytes.is_empty(), "orphaned mapped CP932 bytes");
        return Ok(());
    }
    let text = SHIFT_JIS
        .decode_without_bom_handling_and_without_replacement(cp932_bytes)
        .context("mapped message glyphs are not valid CP932")?
        .into_owned();
    decoded_text.push_str(&text);
    tokens.push(MessageToken::Text {
        source_hex: hex_bytes(source_bytes),
        cp932_hex: hex_bytes(cp932_bytes),
        text,
    });
    source_bytes.clear();
    cp932_bytes.clear();
    Ok(())
}

fn read_mapping_table(
    program: &[u8],
    file_offset: usize,
    entry_count: usize,
) -> Result<Vec<[u8; 2]>> {
    let byte_count = entry_count
        .checked_mul(2)
        .context("glyph mapping table size overflow")?;
    let end = file_offset
        .checked_add(byte_count)
        .context("glyph mapping table offset overflow")?;
    let bytes = program.get(file_offset..end).with_context(|| {
        format!("glyph mapping table at file offset {file_offset:#x} is truncated")
    })?;
    let mut entries = Vec::with_capacity(entry_count);
    for pair in bytes.as_chunks::<2>().0 {
        let encoded = [pair[0], pair[1]];
        ensure!(
            SHIFT_JIS
                .decode_without_bom_handling_and_without_replacement(&encoded)
                .is_some(),
            "glyph mapping table contains invalid CP932 bytes {}",
            hex_bytes(&encoded)
        );
        entries.push(encoded);
    }
    Ok(entries)
}

pub(crate) fn message_format_report() -> MessageFormatReport {
    MessageFormatReport {
        offset_slot_count: MESSAGE_OFFSET_SLOT_COUNT,
        offset_table_size: MESSAGE_OFFSET_TABLE_SIZE,
        record_delimiter_hex: "ff".to_owned(),
        first_single_byte_range: "20-7f".to_owned(),
        first_mapping_table_file_offset: FIRST_GLYPH_TABLE_FILE_OFFSET,
        second_single_byte_range: "a0-df".to_owned(),
        second_mapping_table_file_offset: SECOND_GLYPH_TABLE_FILE_OFFSET,
        direct_cp932_lead_ranges: vec!["80-9f".to_owned(), "e0-fe".to_owned()],
        control_argument_opcodes_hex: [0x00_u8, 0x01, 0x04, 0x05, 0x06, 0x07]
            .into_iter()
            .map(|opcode| format!("{opcode:02x}"))
            .collect(),
    }
}

#[cfg(test)]
#[path = "message_analysis_tests.rs"]
mod tests;
