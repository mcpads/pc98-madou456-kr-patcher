use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result, ensure};
use encoding_rs::SHIFT_JIS;
use serde::{Deserialize, Serialize};

use crate::expected_write::{FixedRangeExpectedWrite, apply_fixed_range_expected_writes};
use crate::localization::inline_text_analysis::encode_two_byte_renderer_text;
use crate::localization::{MADO456_SHA256, sha256_hex};

const UI_TRANSLATION_ASSET: &str = "translations/mado456-ui-ko.json";
const UI_TRANSLATION_SCHEMA: &str = "pc98_madou456.mado456_ui_translation";
const HELD_MONEY_ID: &str = "held_money";
const REMAINING_MOVEMENT_ID: &str = "remaining_movement";
const COUNT_PLACEHOLDER: &str = "{count}";
const HELD_MONEY_SOURCE: &[u8] = &[0x8f, 0x8a, 0x8e, 0x9d, 0x8b, 0xe0];
const HELD_MONEY_FILE_OFFSETS: [usize; 4] = [0x6271, 0x6296, 0x62bb, 0x6e86];
const REMAINING_MOVEMENT_FILE_OFFSET: usize = 0x7660;
const REMAINING_MOVEMENT_COUNT_FILE_OFFSET: usize = 0x7666;
const REMAINING_MOVEMENT_SOURCE: &[u8] = &[
    0x82, 0xcc, 0x82, 0xb1, 0x82, 0xe8, 0x20, 0x20, 0x95, 0xe0, 0x81, 0x41, 0x88, 0xda, 0x93, 0xae,
    0x8f, 0x6f, 0x97, 0x88, 0x82, 0xdc, 0x82, 0xb7, 0xff,
];

#[derive(Debug, Clone, Eq, PartialEq, Serialize)]
pub(crate) struct Mado456UiLocalizationReport {
    pub(crate) asset_sha256: String,
    pub(crate) entry_count: usize,
    pub(crate) translated_occurrence_count: usize,
    pub(crate) fixed_expected_write_count: usize,
    pub(crate) used_external_character_count: usize,
    pub(crate) status: &'static str,
}

pub(crate) struct Mado456UiTranslation {
    entries: BTreeMap<String, String>,
    required_external_characters: BTreeSet<char>,
    asset_sha256: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct UiTranslationAsset {
    schema: String,
    source_program: SourceProgram,
    entries: Vec<UiTranslationEntry>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SourceProgram {
    name: String,
    sha256: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct UiTranslationEntry {
    id: String,
    ko_template: String,
    status: String,
}

impl Mado456UiTranslation {
    pub(crate) fn load() -> Result<Self> {
        let ui_translation_json = crate::private_input::read_text(UI_TRANSLATION_ASSET)?;
        let asset: UiTranslationAsset = serde_json::from_str(ui_translation_json)
            .context("parse embedded MADO456.COM UI translation")?;
        ensure!(
            asset.schema == UI_TRANSLATION_SCHEMA,
            "unsupported MADO456.COM UI translation schema"
        );
        ensure!(
            asset.source_program.name == "MADO456.COM"
                && asset.source_program.sha256 == MADO456_SHA256,
            "MADO456.COM UI translation is not bound to the supported source"
        );

        let mut entries = BTreeMap::new();
        let mut required_external_characters = BTreeSet::new();
        for entry in asset.entries {
            ensure!(
                entry.status == "needs_human_review" || entry.status == "approved",
                "MADO456.COM UI entry {} has an unsupported wording status",
                entry.id
            );
            ensure!(
                !entry.ko_template.is_empty()
                    && !entry.ko_template.chars().any(char::is_control)
                    && !entry.ko_template.chars().any(is_japanese_script),
                "MADO456.COM UI entry {} is empty, controlled, or still Japanese",
                entry.id
            );
            for character in entry.ko_template.chars() {
                if character == '{' || character == '}' || character.is_ascii_alphanumeric() {
                    continue;
                }
                let value = character.to_string();
                if SHIFT_JIS.encode(&value).2 {
                    required_external_characters.insert(character);
                }
            }
            let entry_id = entry.id.clone();
            ensure!(
                entries.insert(entry.id, entry.ko_template).is_none(),
                "MADO456.COM UI translation repeats entry {entry_id}"
            );
        }
        ensure!(
            entries.len() == 2
                && entries.contains_key(HELD_MONEY_ID)
                && entries.contains_key(REMAINING_MOVEMENT_ID),
            "MADO456.COM UI translation population changed"
        );
        ensure!(
            !entries[HELD_MONEY_ID].contains(COUNT_PLACEHOLDER)
                && entries[REMAINING_MOVEMENT_ID]
                    .matches(COUNT_PLACEHOLDER)
                    .count()
                    == 1,
            "MADO456.COM UI dynamic count ownership changed"
        );

        Ok(Self {
            entries,
            required_external_characters,
            asset_sha256: sha256_hex(ui_translation_json.as_bytes()),
        })
    }

    pub(crate) fn required_external_characters(&self) -> &BTreeSet<char> {
        &self.required_external_characters
    }

    pub(crate) fn apply(
        &self,
        routed_program: &[u8],
        codebook: &BTreeMap<char, [u8; 2]>,
    ) -> Result<(Vec<u8>, Mado456UiLocalizationReport, BTreeSet<char>)> {
        let mut used_external_characters = BTreeSet::new();
        let held_money = encode_two_byte_renderer_text(
            &self.entries[HELD_MONEY_ID],
            codebook,
            &mut used_external_characters,
        )?;
        ensure!(
            held_money.len() == HELD_MONEY_SOURCE.len(),
            "translated held-money label changed its six-byte fixed slot"
        );

        let movement_parts = self.entries[REMAINING_MOVEMENT_ID]
            .split(COUNT_PLACEHOLDER)
            .collect::<Vec<_>>();
        ensure!(
            movement_parts.len() == 2,
            "remaining-movement translation changed its dynamic count boundary"
        );
        let mut remaining_movement = encode_two_byte_renderer_text(
            movement_parts[0],
            codebook,
            &mut used_external_characters,
        )?;
        ensure!(
            REMAINING_MOVEMENT_FILE_OFFSET + remaining_movement.len()
                == REMAINING_MOVEMENT_COUNT_FILE_OFFSET,
            "remaining-movement Korean prefix no longer reaches the original count slot"
        );
        remaining_movement.extend_from_slice(&[0x20, 0x20]);
        remaining_movement.extend_from_slice(&encode_two_byte_renderer_text(
            movement_parts[1],
            codebook,
            &mut used_external_characters,
        )?);
        while remaining_movement.len() + 1 < REMAINING_MOVEMENT_SOURCE.len() {
            remaining_movement.extend_from_slice(&[0x81, 0x40]);
        }
        remaining_movement.push(0xff);
        ensure!(
            remaining_movement.len() == REMAINING_MOVEMENT_SOURCE.len(),
            "translated remaining-movement label changed its fixed storage extent"
        );

        let mut writes = HELD_MONEY_FILE_OFFSETS
            .into_iter()
            .map(|offset| FixedRangeExpectedWrite {
                owner: "mado456-game-ui",
                purpose: "translate the player-facing held-money label",
                offset,
                expected_source: HELD_MONEY_SOURCE.to_vec(),
                replacement: held_money.clone(),
            })
            .collect::<Vec<_>>();
        writes.push(FixedRangeExpectedWrite {
            owner: "mado456-game-ui",
            purpose: "translate the remaining-movement prompt while retaining its live count slot",
            offset: REMAINING_MOVEMENT_FILE_OFFSET,
            expected_source: REMAINING_MOVEMENT_SOURCE.to_vec(),
            replacement: remaining_movement,
        });
        let updated = apply_fixed_range_expected_writes(routed_program, &writes)?;
        ensure!(
            updated[REMAINING_MOVEMENT_COUNT_FILE_OFFSET..REMAINING_MOVEMENT_COUNT_FILE_OFFSET + 2]
                == [0x20, 0x20],
            "remaining-movement live count slot changed during translation"
        );
        Ok((
            updated,
            Mado456UiLocalizationReport {
                asset_sha256: self.asset_sha256.clone(),
                entry_count: self.entries.len(),
                translated_occurrence_count: writes.len(),
                fixed_expected_write_count: writes.len(),
                used_external_character_count: used_external_characters.len(),
                status: "all verified MADO456.COM game-UI literals are translated in fixed storage; wording remains human-review pending",
            },
            used_external_characters,
        ))
    }
}

fn is_japanese_script(character: char) -> bool {
    matches!(
        character,
        '\u{3040}'..='\u{30ff}' | '\u{31f0}'..='\u{31ff}' | '\u{ff66}'..='\u{ff9f}'
    )
}

#[cfg(test)]
#[path = "mado456_ui_tests.rs"]
mod tests;
