use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};

use crate::reassembly::load_verified_game_files;

use super::{catalog_inline_text, catalog_messages, sha256_hex};

const TRANSLATION_DRAFT_SCHEMA: &str = "pc98_madou456.translation_draft";
const MESSAGE_COLLECTION: &str = "messages";
const INLINE_TEXT_COLLECTION: &str = "inline_text";
const UNTRANSLATED_STATUS: &str = "untranslated";
const PRESERVE_SOURCE_STATUS: &str = "preserve_source";
const IN_PROGRESS_STATUS: &str = "in_progress";
const NEEDS_REVIEW_STATUS: &str = "needs_review";
const NEEDS_HUMAN_REVIEW_STATUS: &str = "needs_human_review";
const APPROVED_STATUS: &str = "approved";

#[derive(Debug, Serialize)]
pub struct TranslationDraftAuditReport {
    draft: PathBuf,
    draft_sha256: String,
    message_catalog_sha256: String,
    message_entry_count: usize,
    inline_text_catalog_sha256: String,
    inline_text_entry_count: usize,
    entry_count: usize,
    untranslated_entry_count: usize,
    preserve_source_entry_count: usize,
    in_progress_entry_count: usize,
    needs_review_entry_count: usize,
    needs_human_review_entry_count: usize,
    approved_entry_count: usize,
    status: &'static str,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TranslationDraft {
    schema: String,
    source_catalogs: Vec<SourceCatalogBinding>,
    collections: Vec<TranslationCollection>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct SourceCatalogBinding {
    id: String,
    sha256: String,
    entry_count: usize,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct TranslationCollection {
    id: String,
    entries: Vec<TranslationEntry>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct TranslationEntry {
    id: String,
    ko_segments: Vec<String>,
    status: String,
    notes: String,
}

pub(crate) fn build_translation_draft<'a>(
    message_catalog_sha256: &str,
    message_layouts: impl IntoIterator<Item = (&'a str, usize)>,
    inline_text_catalog_sha256: &str,
    inline_text_layouts: impl IntoIterator<Item = (&'a str, usize)>,
) -> Result<TranslationDraft> {
    let message_layouts: Vec<(String, usize)> = message_layouts
        .into_iter()
        .map(|(id, segment_count)| (id.to_owned(), segment_count))
        .collect();
    let inline_text_layouts: Vec<(String, usize)> = inline_text_layouts
        .into_iter()
        .map(|(id, segment_count)| (id.to_owned(), segment_count))
        .collect();
    let draft = TranslationDraft {
        schema: TRANSLATION_DRAFT_SCHEMA.to_owned(),
        source_catalogs: vec![
            source_binding(
                MESSAGE_COLLECTION,
                message_catalog_sha256,
                message_layouts.len(),
            ),
            source_binding(
                INLINE_TEXT_COLLECTION,
                inline_text_catalog_sha256,
                inline_text_layouts.len(),
            ),
        ],
        collections: vec![
            translation_collection(MESSAGE_COLLECTION, message_layouts.clone())?,
            translation_collection(INLINE_TEXT_COLLECTION, inline_text_layouts.clone())?,
        ],
    };
    audit_translation_draft(
        &draft,
        message_catalog_sha256,
        message_layouts
            .iter()
            .map(|(id, segment_count)| (id.as_str(), *segment_count)),
        inline_text_catalog_sha256,
        inline_text_layouts
            .iter()
            .map(|(id, segment_count)| (id.as_str(), *segment_count)),
    )?;
    Ok(draft)
}

pub(crate) fn audit_translation_draft<'a>(
    draft: &TranslationDraft,
    message_catalog_sha256: &str,
    message_layouts: impl IntoIterator<Item = (&'a str, usize)>,
    inline_text_catalog_sha256: &str,
    inline_text_layouts: impl IntoIterator<Item = (&'a str, usize)>,
) -> Result<()> {
    ensure!(
        draft.schema == TRANSLATION_DRAFT_SCHEMA,
        "translation draft has an unsupported schema"
    );
    let expected = [
        (
            MESSAGE_COLLECTION,
            message_catalog_sha256,
            message_layouts.into_iter().collect::<Vec<_>>(),
        ),
        (
            INLINE_TEXT_COLLECTION,
            inline_text_catalog_sha256,
            inline_text_layouts.into_iter().collect::<Vec<_>>(),
        ),
    ];
    ensure!(
        draft.source_catalogs.len() == expected.len() && draft.collections.len() == expected.len(),
        "translation draft must contain exactly the message and inline-text collections"
    );

    for (index, (expected_id, expected_sha256, expected_layouts)) in expected.iter().enumerate() {
        let binding = &draft.source_catalogs[index];
        ensure!(
            binding.id == *expected_id
                && binding.sha256 == *expected_sha256
                && binding.entry_count == expected_layouts.len(),
            "translation draft source binding {expected_id} changed"
        );
        let collection = &draft.collections[index];
        ensure!(
            collection.id == *expected_id && collection.entries.len() == expected_layouts.len(),
            "translation draft collection {expected_id} changed its identity or population"
        );
        let mut seen_ids = BTreeSet::new();
        for (entry, (expected_entry_id, expected_segment_count)) in
            collection.entries.iter().zip(expected_layouts)
        {
            ensure!(
                entry.id == **expected_entry_id && seen_ids.insert(entry.id.as_str()),
                "translation draft collection {expected_id} changed, reordered, or duplicated an entry id"
            );
            ensure!(
                entry.ko_segments.len() == *expected_segment_count,
                "translation draft entry {} changed its source-owned text segment count",
                entry.id
            );
            validate_translation_fields(
                expected_id,
                &entry.id,
                &entry.ko_segments,
                &entry.status,
                &entry.notes,
            )?;
        }
    }
    Ok(())
}

pub fn audit_verified_translation_draft(
    source_cd_path: &Path,
    system_hdi_path: &Path,
    draft_path: &Path,
) -> Result<TranslationDraftAuditReport> {
    let files = load_verified_game_files(source_cd_path, system_hdi_path)?;
    let message_catalog = catalog_messages(&files)?;
    let inline_text_catalog = catalog_inline_text(&files)?;
    let message_catalog_sha256 = serialized_sha256(&message_catalog)?;
    let inline_text_catalog_sha256 = serialized_sha256(&inline_text_catalog)?;
    let draft_bytes = fs::read(draft_path)
        .with_context(|| format!("could not read translation draft {}", draft_path.display()))?;
    let draft: TranslationDraft = serde_json::from_slice(&draft_bytes)
        .with_context(|| format!("could not parse translation draft {}", draft_path.display()))?;

    audit_translation_draft(
        &draft,
        &message_catalog_sha256,
        message_catalog.translation_layouts(),
        &inline_text_catalog_sha256,
        inline_text_catalog.translation_layouts(),
    )?;
    let untranslated_entry_count = draft.status_count(UNTRANSLATED_STATUS);
    let preserve_source_entry_count = draft.status_count(PRESERVE_SOURCE_STATUS);
    let in_progress_entry_count = draft.status_count(IN_PROGRESS_STATUS);
    let needs_review_entry_count = draft.status_count(NEEDS_REVIEW_STATUS);
    let needs_human_review_entry_count = draft.status_count(NEEDS_HUMAN_REVIEW_STATUS);
    let approved_entry_count = draft.status_count(APPROVED_STATUS);
    let entry_count = draft.entry_count();
    ensure!(
        untranslated_entry_count
            + preserve_source_entry_count
            + in_progress_entry_count
            + needs_review_entry_count
            + needs_human_review_entry_count
            + approved_entry_count
            == entry_count,
        "translation draft status population changed after audit"
    );

    Ok(TranslationDraftAuditReport {
        draft: draft_path.to_path_buf(),
        draft_sha256: sha256_hex(&draft_bytes),
        message_catalog_sha256,
        message_entry_count: message_catalog.message_entry_count,
        inline_text_catalog_sha256,
        inline_text_entry_count: inline_text_catalog.analysis.entry_count,
        entry_count,
        untranslated_entry_count,
        preserve_source_entry_count,
        in_progress_entry_count,
        needs_review_entry_count,
        needs_human_review_entry_count,
        approved_entry_count,
        status: "static draft structure, wording states, and empty-segment source replay verified; layout, glyph coverage, translation encoding, file reinsertion, presentation, runtime display, build eligibility, and distribution eligibility are separate and not proven",
    })
}

impl TranslationDraft {
    pub(crate) fn entry_count(&self) -> usize {
        self.collections
            .iter()
            .map(|collection| collection.entries.len())
            .sum()
    }

    fn status_count(&self, status: &str) -> usize {
        self.collections
            .iter()
            .flat_map(|collection| &collection.entries)
            .filter(|entry| entry.status == status)
            .count()
    }

    pub(crate) fn collection_segments(
        &self,
        collection_id: &str,
    ) -> Result<BTreeMap<&str, &[String]>> {
        let collection = self
            .collections
            .iter()
            .find(|collection| collection.id == collection_id)
            .with_context(|| format!("translation draft lacks collection {collection_id}"))?;
        Ok(collection
            .entries
            .iter()
            .map(|entry| (entry.id.as_str(), entry.ko_segments.as_slice()))
            .collect())
    }

    pub(crate) fn apply_translation(
        &mut self,
        collection_id: &str,
        entry_id: &str,
        ko_segments: &[String],
        status: &str,
        notes: &str,
    ) -> Result<()> {
        validate_translation_fields(collection_id, entry_id, ko_segments, status, notes)?;
        let collection = self
            .collections
            .iter_mut()
            .find(|collection| collection.id == collection_id)
            .with_context(|| format!("translation draft lacks collection {collection_id}"))?;
        let entry = collection
            .entries
            .iter_mut()
            .find(|entry| entry.id == entry_id)
            .with_context(|| {
                format!("translation draft collection {collection_id} lacks entry {entry_id}")
            })?;
        ensure!(
            entry.ko_segments.len() == ko_segments.len(),
            "translation overlay changed source-owned text segment count for {entry_id}"
        );
        entry.ko_segments.clone_from_slice(ko_segments);
        status.clone_into(&mut entry.status);
        notes.clone_into(&mut entry.notes);
        Ok(())
    }
}

pub(crate) fn validate_translation_fields(
    collection_id: &str,
    entry_id: &str,
    ko_segments: &[String],
    status: &str,
    notes: &str,
) -> Result<()> {
    ensure!(
        ko_segments.iter().all(|segment| {
            !segment.contains(['\0', '\r'])
                && (collection_id == MESSAGE_COLLECTION || !segment.contains('\n'))
        }) && !notes.contains('\0'),
        "translation entry {entry_id} contains an unsupported control character"
    );
    for segment in ko_segments {
        if collection_id == MESSAGE_COLLECTION {
            crate::josa::manual_message_lines(segment).with_context(|| {
                format!("translation entry {entry_id} has invalid manual line layout")
            })?;
        } else {
            crate::josa::runtime_text_characters(segment).with_context(|| {
                format!("translation entry {entry_id} has invalid particle syntax")
            })?;
        }
    }
    let has_translation = ko_segments.iter().any(|segment| !segment.is_empty());
    match status {
        UNTRANSLATED_STATUS => ensure!(
            !has_translation,
            "untranslated entry {entry_id} must have empty Korean text"
        ),
        PRESERVE_SOURCE_STATUS => ensure!(
            !has_translation && !notes.trim().is_empty(),
            "source-preserved entry {entry_id} must have empty Korean text and a reason in notes"
        ),
        IN_PROGRESS_STATUS | NEEDS_REVIEW_STATUS | NEEDS_HUMAN_REVIEW_STATUS | APPROVED_STATUS => {
            ensure!(
                has_translation,
                "translated entry {entry_id} must have nonempty Korean text"
            )
        }
        _ => ensure!(false, "entry {entry_id} has an unsupported status"),
    }
    Ok(())
}

pub(crate) fn serialized_sha256<T: Serialize>(value: &T) -> Result<String> {
    let mut encoded = serde_json::to_vec_pretty(value)?;
    encoded.push(b'\n');
    Ok(sha256_hex(&encoded))
}

fn source_binding(id: &str, sha256: &str, entry_count: usize) -> SourceCatalogBinding {
    SourceCatalogBinding {
        id: id.to_owned(),
        sha256: sha256.to_owned(),
        entry_count,
    }
}

fn translation_collection(
    id: &str,
    layouts: Vec<(String, usize)>,
) -> Result<TranslationCollection> {
    let mut entries = Vec::with_capacity(layouts.len());
    for (entry_id, segment_count) in layouts {
        ensure!(
            segment_count > 0,
            "translation draft entry {entry_id} has no source text segment"
        );
        entries.push(TranslationEntry {
            id: entry_id,
            ko_segments: vec![String::new(); segment_count],
            status: UNTRANSLATED_STATUS.to_owned(),
            notes: String::new(),
        });
    }
    Ok(TranslationCollection {
        id: id.to_owned(),
        entries,
    })
}

#[cfg(test)]
#[path = "translation_draft_tests.rs"]
mod tests;
