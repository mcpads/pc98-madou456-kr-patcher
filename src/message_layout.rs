use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, ensure};
use serde::Serialize;

use super::message_analysis::MessageToken;
use super::translation_draft::{build_translation_draft, serialized_sha256};
use super::translation_overlay::load_translation_overlay_catalog;
use super::{catalog_inline_text, catalog_messages, load_verified_game_files};

const MESSAGE_COLLECTION: &str = "messages";

// MADO456.COM runtime 0x33C3 draws these messages with BX=0x010B/0x110B
// and CX=0x0416. The frame has 22 middle cells, with relative columns 1..=23
// available to message text. The two proven callers cover the opening ceremony
// (0x3359) and the start countdown (0x354C).
const CONFIRMED_OPENING_MESSAGE_PANE: MessagePaneProfile = MessagePaneProfile {
    id: "confirmed_opening_message_pane",
    first_text_column: 1,
    last_text_column: 23,
    first_text_row: 1,
    last_text_row: 4,
    evidence_level: "source_proven_call_site",
    geometry_basis: "MADO456.COM runtime 0x33C3 and 0x354C use the 23-cell opening pane",
};

const STANDARD_DIALOGUE_MESSAGE_FAMILY: MessagePaneProfile = MessagePaneProfile {
    id: "standard_dialogue_message_family",
    first_text_column: 1,
    last_text_column: 23,
    first_text_row: 1,
    last_text_row: 4,
    evidence_level: "static_family_candidate",
    geometry_basis: "MSGEV.DAT and MSG01.DAT through MSG13.DAT are adjacent event or character-message families whose source text stays within the shared 23-cell pane envelope",
};

const EVENT_HELP_MESSAGE_FAMILY: MessagePaneProfile = MessagePaneProfile {
    id: "event_help_message_family",
    first_text_column: 1,
    last_text_column: 23,
    first_text_row: 1,
    last_text_row: 5,
    evidence_level: "static_family_candidate",
    geometry_basis: "adjacent MSGEV.DAT:AA-D9 help records share a source-authored five-row layout within the 23-cell message envelope",
};

const COMMON_QUOTED_DIALOGUE: MessagePaneProfile = MessagePaneProfile {
    id: "common_quoted_dialogue",
    first_text_column: 1,
    last_text_column: 23,
    first_text_row: 1,
    last_text_row: 3,
    evidence_level: "static_content_candidate",
    geometry_basis: "MSG.DAT entry contains the source dialogue opener and stays within the standard 23-cell message envelope",
};

const COMMON_QUOTED_FIVE_ROW_DIALOGUE: MessagePaneProfile = MessagePaneProfile {
    id: "common_quoted_five_row_dialogue",
    first_text_column: 1,
    last_text_column: 23,
    first_text_row: 1,
    last_text_row: 5,
    evidence_level: "static_content_candidate",
    geometry_basis: "adjacent MSG.DAT:62-63 quoted records use a source-authored five-row message layout",
};

const MERCHANT_DIALOGUE: MessagePaneProfile = MessagePaneProfile {
    id: "merchant_dialogue_region",
    first_text_column: 1,
    last_text_column: 18,
    first_text_row: 1,
    last_text_row: 3,
    evidence_level: "authored_source_envelope_candidate",
    geometry_basis: "adjacent MSG.DAT:C8-DB merchant records author and clear an 18-cell text region",
};

const QUEST_DESCRIPTION: MessagePaneProfile = MessagePaneProfile {
    id: "quest_description_region",
    first_text_column: 1,
    last_text_column: 18,
    first_text_row: 1,
    last_text_row: 2,
    evidence_level: "authored_source_envelope_candidate",
    geometry_basis: "adjacent MSG.DAT:F0-FA quest-description records align their source lines to an 18-cell region",
};

#[derive(Clone, Copy, Debug)]
struct MessagePaneProfile {
    id: &'static str,
    first_text_column: usize,
    last_text_column: usize,
    first_text_row: usize,
    last_text_row: usize,
    evidence_level: &'static str,
    geometry_basis: &'static str,
}

#[derive(Debug, Serialize)]
pub struct MessageOverflowDetectionReport {
    translation_catalog: PathBuf,
    translation_catalog_sha256: String,
    profile_count: usize,
    profiles: Vec<MessagePaneProfileSummary>,
    covered_entry_count: usize,
    checked_segment_count: usize,
    unprofiled_message_entry_count: usize,
    horizontal_overflow_candidate_count: usize,
    horizontal_overflow_candidates: Vec<MessageHorizontalOverflow>,
    vertical_overflow_candidate_count: usize,
    vertical_overflow_candidates: Vec<MessageVerticalOverflow>,
    automatic_line_breaking_performed: bool,
    translation_changed_by_detector: bool,
    status: &'static str,
}

#[derive(Debug, Serialize)]
struct MessagePaneProfileSummary {
    id: &'static str,
    evidence_level: &'static str,
    geometry_basis: &'static str,
    first_text_column: usize,
    last_text_column: usize,
    first_text_row: usize,
    last_text_row: usize,
    covered_entry_count: usize,
    checked_segment_count: usize,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
struct MessageHorizontalOverflow {
    profile_id: &'static str,
    evidence_level: &'static str,
    entry_id: String,
    segment_index: usize,
    start_column: usize,
    cell_count: usize,
    source_cell_count: usize,
    available_cell_count: usize,
    overflow_cell_count: usize,
    translated_text: String,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
struct MessageVerticalOverflow {
    profile_id: &'static str,
    evidence_level: &'static str,
    entry_id: String,
    segment_index: usize,
    manual_line_index: usize,
    text_row: usize,
    last_text_row: usize,
    overflow_row_count: usize,
    translated_text: String,
}

pub fn detect_verified_message_overflow(
    source_cd_path: &Path,
    system_hdi_path: &Path,
    translation_catalog_path: &Path,
) -> Result<MessageOverflowDetectionReport> {
    let files = load_verified_game_files(source_cd_path, system_hdi_path)?;
    let message_catalog = catalog_messages(&files)?;
    let inline_text_catalog = catalog_inline_text(&files)?;
    let message_catalog_sha256 = serialized_sha256(&message_catalog)?;
    let inline_text_catalog_sha256 = serialized_sha256(&inline_text_catalog)?;
    let mut draft = build_translation_draft(
        &message_catalog_sha256,
        message_catalog.translation_layouts(),
        &inline_text_catalog_sha256,
        inline_text_catalog.translation_layouts(),
    )?;
    let loaded_translation_catalog = load_translation_overlay_catalog(
        translation_catalog_path,
        &message_catalog_sha256,
        &message_catalog,
        &inline_text_catalog_sha256,
        &inline_text_catalog,
    )?;
    loaded_translation_catalog.apply_to_draft(&mut draft)?;
    ensure!(
        loaded_translation_catalog.entry_count == draft.entry_count(),
        "translation catalog does not own every extracted text entry"
    );

    let message_translations = draft.collection_segments(MESSAGE_COLLECTION)?;
    let mut checked_segment_count = 0usize;
    let mut covered_entry_count = 0usize;
    let mut profile_counts = BTreeMap::<&str, (MessagePaneProfile, usize, usize)>::new();
    let mut horizontal_overflows = Vec::new();
    let mut vertical_overflows = Vec::new();
    for (entry_id, tokens) in message_catalog.translation_entries() {
        let Some(profile) = message_profile(entry_id, tokens)? else {
            continue;
        };
        let segments = message_translations
            .get(entry_id)
            .with_context(|| format!("translation draft lacks profiled message {entry_id}"))?;
        covered_entry_count += 1;
        checked_segment_count += segments.len();
        let counts = profile_counts.entry(profile.id).or_insert((profile, 0, 0));
        counts.1 += 1;
        counts.2 += segments.len();
        let findings = detect_message_overflows(entry_id, tokens, segments, profile)?;
        horizontal_overflows.extend(findings.horizontal);
        vertical_overflows.extend(findings.vertical);
    }
    let profiles = profile_counts
        .into_values()
        .map(
            |(profile, covered_entry_count, checked_segment_count)| MessagePaneProfileSummary {
                id: profile.id,
                evidence_level: profile.evidence_level,
                geometry_basis: profile.geometry_basis,
                first_text_column: profile.first_text_column,
                last_text_column: profile.last_text_column,
                first_text_row: profile.first_text_row,
                last_text_row: profile.last_text_row,
                covered_entry_count,
                checked_segment_count,
            },
        )
        .collect::<Vec<_>>();

    Ok(MessageOverflowDetectionReport {
        translation_catalog: loaded_translation_catalog.catalog,
        translation_catalog_sha256: loaded_translation_catalog.catalog_sha256,
        profile_count: profiles.len(),
        profiles,
        covered_entry_count,
        checked_segment_count,
        unprofiled_message_entry_count: message_catalog
            .message_entry_count
            .saturating_sub(covered_entry_count),
        horizontal_overflow_candidate_count: horizontal_overflows.len(),
        horizontal_overflow_candidates: horizontal_overflows,
        vertical_overflow_candidate_count: vertical_overflows.len(),
        vertical_overflow_candidates: vertical_overflows,
        automatic_line_breaking_performed: false,
        translation_changed_by_detector: false,
        status: "horizontal and vertical overflow candidates were checked across source-proven and statically grouped dialogue-like message families; only manually authored line breaks affect layout, no automatic wrapping occurred, candidate evidence levels remain distinct, and dynamic composition, presentation, and runtime behavior remain unproven",
    })
}

fn message_profile(entry_id: &str, tokens: &[MessageToken]) -> Result<Option<MessagePaneProfile>> {
    let (file, slot_hex) = entry_id
        .split_once(':')
        .with_context(|| format!("message entry id {entry_id} lacks a slot separator"))?;
    let slot = usize::from_str_radix(slot_hex, 16)
        .with_context(|| format!("message entry id {entry_id} has an invalid hexadecimal slot"))?;

    if file == "MSGEV.DAT" && is_confirmed_opening_slot(slot) {
        return Ok(Some(CONFIRMED_OPENING_MESSAGE_PANE));
    }
    if file == "MSGEV.DAT" && (0xaa..=0xd9).contains(&slot) {
        return Ok(Some(EVENT_HELP_MESSAGE_FAMILY));
    }
    if file == "MSGEV.DAT" || is_character_message_file(file) {
        return Ok(Some(STANDARD_DIALOGUE_MESSAGE_FAMILY));
    }
    if file != "MSG.DAT" || !contains_source_dialogue_opener(tokens) {
        return Ok(None);
    }
    if (0xc8..=0xdb).contains(&slot) {
        return Ok(Some(MERCHANT_DIALOGUE));
    }
    if (0xf0..=0xfa).contains(&slot) {
        return Ok(Some(QUEST_DESCRIPTION));
    }
    if (0x62..=0x63).contains(&slot) {
        return Ok(Some(COMMON_QUOTED_FIVE_ROW_DIALOGUE));
    }
    Ok(Some(COMMON_QUOTED_DIALOGUE))
}

fn is_confirmed_opening_slot(slot: usize) -> bool {
    (0x14..=0x1f).contains(&slot) || (0x22..=0x24).contains(&slot)
}

fn is_character_message_file(file: &str) -> bool {
    let Some(number) = file
        .strip_prefix("MSG")
        .and_then(|suffix| suffix.strip_suffix(".DAT"))
    else {
        return false;
    };
    number
        .parse::<usize>()
        .is_ok_and(|number| (1..=13).contains(&number))
}

fn contains_source_dialogue_opener(tokens: &[MessageToken]) -> bool {
    tokens.iter().any(|token| match token {
        MessageToken::Text { text, .. } => text.contains('「'),
        MessageToken::Control { .. } => false,
    })
}

#[derive(Debug, Default)]
struct MessageOverflowFindings {
    horizontal: Vec<MessageHorizontalOverflow>,
    vertical: Vec<MessageVerticalOverflow>,
}

fn detect_message_overflows(
    entry_id: &str,
    tokens: &[MessageToken],
    translated_segments: &[String],
    profile: MessagePaneProfile,
) -> Result<MessageOverflowFindings> {
    let mut base_column = None;
    let mut current_column = None;
    let mut source_column = None;
    let mut current_row = None;
    let mut source_row = None;
    let mut segment_index = 0usize;
    let mut findings = MessageOverflowFindings::default();

    for token in tokens {
        match token {
            MessageToken::Control {
                opcode_hex,
                argument_hex,
            } if opcode_hex == "00" => {
                let column = parse_control_argument(entry_id, opcode_hex, argument_hex)?;
                base_column = Some(column);
                current_column = Some(column);
                source_column = Some(column);
            }
            MessageToken::Control {
                opcode_hex,
                argument_hex,
            } if opcode_hex == "01" => {
                let row = parse_control_argument(entry_id, opcode_hex, argument_hex)?;
                current_row = Some(row);
                source_row = Some(row);
            }
            MessageToken::Control { opcode_hex, .. } if opcode_hex == "02" => {
                current_column = base_column;
                source_column = base_column;
                current_row = current_row.map(|row| row.saturating_add(1));
                source_row = source_row.map(|row| row.saturating_add(1));
            }
            MessageToken::Text {
                text: source_text, ..
            } => {
                let text = translated_segments.get(segment_index).with_context(|| {
                    format!("profiled message {entry_id} lacks translated segment {segment_index}")
                })?;
                let start_column = current_column.with_context(|| {
                    format!("profiled message {entry_id} has text before a column control")
                })?;
                let source_cell_count = source_text.chars().count();
                let source_start_column = source_column.with_context(|| {
                    format!("profiled message {entry_id} has source text before a column control")
                })?;
                let source_text_row = source_row.with_context(|| {
                    format!("profiled message {entry_id} has source text before a row control")
                })?;
                ensure!(
                    source_start_column >= profile.first_text_column
                        && source_start_column.saturating_add(source_cell_count)
                            <= profile.last_text_column.saturating_add(1),
                    "source message {entry_id} segment {segment_index} does not fit profile {}",
                    profile.id
                );
                ensure!(
                    source_text_row >= profile.first_text_row
                        && source_text_row <= profile.last_text_row,
                    "source message {entry_id} segment {segment_index} does not fit profile {} rows",
                    profile.id
                );

                let lines = crate::josa::manual_message_lines(text)?;
                let mut translated_column = start_column;
                let mut translated_row = current_row.with_context(|| {
                    format!("profiled message {entry_id} has text before a row control")
                })?;
                for (manual_line_index, line) in lines.iter().enumerate() {
                    if manual_line_index > 0 {
                        translated_column = base_column.with_context(|| {
                            format!("profiled message {entry_id} line break lacks a base column")
                        })?;
                        translated_row = translated_row.saturating_add(1);
                    }
                    let cell_count = crate::josa::runtime_text_characters(line)?.len();
                    let available_cell_count = if translated_column < profile.first_text_column {
                        0
                    } else {
                        profile
                            .last_text_column
                            .saturating_add(1)
                            .saturating_sub(translated_column)
                    };
                    let overflow_cell_count = cell_count.saturating_sub(available_cell_count);
                    if overflow_cell_count > 0 {
                        findings.horizontal.push(MessageHorizontalOverflow {
                            profile_id: profile.id,
                            evidence_level: profile.evidence_level,
                            entry_id: entry_id.to_owned(),
                            segment_index,
                            start_column: translated_column,
                            cell_count,
                            source_cell_count,
                            available_cell_count,
                            overflow_cell_count,
                            translated_text: (*line).to_owned(),
                        });
                    }
                    if translated_row > profile.last_text_row {
                        findings.vertical.push(MessageVerticalOverflow {
                            profile_id: profile.id,
                            evidence_level: profile.evidence_level,
                            entry_id: entry_id.to_owned(),
                            segment_index,
                            manual_line_index,
                            text_row: translated_row,
                            last_text_row: profile.last_text_row,
                            overflow_row_count: translated_row - profile.last_text_row,
                            translated_text: (*line).to_owned(),
                        });
                    }
                    translated_column = translated_column.saturating_add(cell_count);
                }
                current_column = Some(translated_column);
                current_row = Some(translated_row);
                source_column = Some(source_start_column.saturating_add(source_cell_count));
                segment_index += 1;
            }
            MessageToken::Control { .. } => {}
        }
    }

    ensure!(
        segment_index == translated_segments.len(),
        "profiled message {entry_id} translated segment population differs from its tokens"
    );
    Ok(findings)
}

fn parse_control_argument(
    entry_id: &str,
    opcode_hex: &str,
    argument_hex: &Option<String>,
) -> Result<usize> {
    let argument_hex = argument_hex.as_deref().with_context(|| {
        format!("profiled message {entry_id} control {opcode_hex} lacks an argument")
    })?;
    usize::from_str_radix(argument_hex, 16).with_context(|| {
        format!("profiled message {entry_id} control {opcode_hex} has invalid argument")
    })
}

#[cfg(test)]
#[path = "message_layout_tests.rs"]
mod tests;
