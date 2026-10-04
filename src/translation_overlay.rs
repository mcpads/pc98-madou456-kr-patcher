use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail, ensure};
use encoding_rs::SHIFT_JIS;
use serde::{Deserialize, Serialize};

use crate::reassembly::load_verified_game_files;

use super::translation_context::audit_translation_context_assignments;
use super::translation_draft::{TranslationDraft, serialized_sha256, validate_translation_fields};
use super::{InlineTextCatalog, MessageCatalog, catalog_inline_text, catalog_messages, sha256_hex};

const OVERLAY_SCHEMA: &str = "pc98_madou456.translation_overlay";
const OVERLAY_CATALOG_SCHEMA: &str = "pc98_madou456.translation_overlay_catalog";
const REPRESENTATIVE_SELECTION_SCHEMA: &str = "pc98_madou456.translation_representative_sample";
const BATCH_SELECTION_SCHEMA: &str = "pc98_madou456.translation_batch_selection";
const REPRESENTATIVE_SCOPE: &str = "representative_first_draft";
const CONTEXT_BATCH_SCOPE: &str = "context_batch";
const MESSAGE_COLLECTION: &str = "messages";
const INLINE_TEXT_COLLECTION: &str = "inline_text";
const PRESERVE_SOURCE_STATUS: &str = "preserve_source";
const STATIC_EXTERNAL_CHARACTER_CAPACITY: usize = 188;

#[derive(Debug, Serialize)]
pub struct TranslationOverlayAuditReport {
    overlay: PathBuf,
    overlay_sha256: String,
    contexts: PathBuf,
    contexts_sha256: String,
    context_assignment_count: usize,
    selection: PathBuf,
    selection_sha256: String,
    message_catalog_sha256: String,
    inline_text_catalog_sha256: String,
    entry_count: usize,
    translated_entry_count: usize,
    preserve_source_entry_count: usize,
    translated_segment_count: usize,
    translated_character_count: usize,
    unique_non_whitespace_character_count: usize,
    required_external_character_count: usize,
    required_external_characters: Vec<String>,
    static_external_character_capacity: usize,
    fits_static_external_character_capacity: bool,
    longer_than_source_segment_count: usize,
    maximum_segment_cell_count: usize,
    maximum_positive_cell_growth: usize,
    longer_than_source_segments: Vec<String>,
    status: &'static str,
}

#[derive(Debug, Serialize)]
pub struct TranslationOverlayCatalogAuditReport {
    catalog: PathBuf,
    catalog_sha256: String,
    contexts: PathBuf,
    contexts_sha256: String,
    context_assignment_count: usize,
    message_catalog_sha256: String,
    inline_text_catalog_sha256: String,
    batch_count: usize,
    entry_count: usize,
    translated_entry_count: usize,
    preserve_source_entry_count: usize,
    translated_segment_count: usize,
    translated_character_count: usize,
    unique_non_whitespace_character_count: usize,
    required_external_character_count: usize,
    required_external_characters: Vec<String>,
    static_external_character_capacity: usize,
    fits_static_external_character_capacity: bool,
    longer_than_source_segment_count: usize,
    maximum_segment_cell_count: usize,
    maximum_positive_cell_growth: usize,
    longer_than_source_segments: Vec<String>,
    status: &'static str,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TranslationOverlayCatalog {
    schema: String,
    source_catalogs: Vec<SourceCatalogBinding>,
    contexts: CatalogFileBinding,
    batches: Vec<TranslationBatchBinding>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CatalogFileBinding {
    path: PathBuf,
    sha256: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TranslationBatchBinding {
    id: String,
    selection: PathBuf,
    selection_sha256: String,
    overlay: PathBuf,
    overlay_sha256: String,
    entry_count: usize,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TranslationOverlay {
    schema: String,
    scope: String,
    source_catalogs: Vec<SourceCatalogBinding>,
    selection: SelectionBinding,
    entries: Vec<TranslationOverlayEntry>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SourceCatalogBinding {
    id: String,
    sha256: String,
    entry_count: usize,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SelectionBinding {
    id: String,
    sha256: String,
    entry_count: usize,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TranslationOverlayEntry {
    collection: String,
    id: String,
    ko_segments: Vec<String>,
    status: String,
    notes: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TranslationSelection {
    schema: String,
    context_catalog_sha256: String,
    #[serde(default)]
    selection_status: Option<String>,
    #[serde(default)]
    batch_id: Option<String>,
    entries: Vec<RepresentativeSelectionEntry>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RepresentativeSelectionEntry {
    id: String,
    context_id: String,
    goals: Vec<String>,
}

pub(crate) struct LoadedTranslationOverlayCatalog {
    pub(crate) catalog: PathBuf,
    pub(crate) catalog_sha256: String,
    pub(crate) entry_count: usize,
    contexts: PathBuf,
    contexts_sha256: String,
    context_assignment_count: usize,
    batch_count: usize,
    audit: OverlayAudit,
    overlays: Vec<TranslationOverlay>,
}

#[derive(Default)]
struct OverlayAudit {
    translated_entry_count: usize,
    preserve_source_entry_count: usize,
    translated_segment_count: usize,
    translated_character_count: usize,
    unique_non_whitespace_characters: BTreeSet<char>,
    required_external_characters: BTreeSet<char>,
    longer_than_source_segments: Vec<String>,
    maximum_segment_cell_count: usize,
    maximum_positive_cell_growth: usize,
}

struct OverlaySourceContext<'a> {
    contexts_sha256: &'a str,
    context_assignments: &'a BTreeMap<String, String>,
    message_catalog_sha256: &'a str,
    message_source_cell_counts: &'a BTreeMap<String, Vec<usize>>,
    inline_text_catalog_sha256: &'a str,
    inline_text_source_cell_counts: &'a BTreeMap<String, Vec<usize>>,
}

pub fn audit_verified_translation_overlay(
    source_cd_path: &Path,
    system_hdi_path: &Path,
    contexts_path: &Path,
    selection_path: &Path,
    overlay_path: &Path,
) -> Result<TranslationOverlayAuditReport> {
    let files = load_verified_game_files(source_cd_path, system_hdi_path)?;
    let message_catalog = catalog_messages(&files)?;
    let inline_text_catalog = catalog_inline_text(&files)?;
    let message_catalog_sha256 = serialized_sha256(&message_catalog)?;
    let inline_text_catalog_sha256 = serialized_sha256(&inline_text_catalog)?;
    let message_ids = message_catalog
        .translation_layouts()
        .map(|(id, _)| id.to_owned())
        .collect::<Vec<_>>();
    let inline_text_ids = inline_text_catalog
        .translation_layouts()
        .map(|(id, _)| id.to_owned())
        .collect::<Vec<_>>();
    let (contexts_sha256, context_assignments) = audit_translation_context_assignments(
        contexts_path,
        &message_catalog_sha256,
        &message_ids,
        &inline_text_catalog_sha256,
        &inline_text_ids,
    )?;
    let (selection, selection_bytes) = read_selection(selection_path)?;
    let (overlay, overlay_bytes) = read_overlay(overlay_path)?;
    let message_source_cell_counts = message_catalog.translation_source_cell_counts();
    let inline_text_source_cell_counts = inline_text_catalog.translation_source_cell_counts();
    let source_context = OverlaySourceContext {
        contexts_sha256: &contexts_sha256,
        context_assignments: &context_assignments,
        message_catalog_sha256: &message_catalog_sha256,
        message_source_cell_counts: &message_source_cell_counts,
        inline_text_catalog_sha256: &inline_text_catalog_sha256,
        inline_text_source_cell_counts: &inline_text_source_cell_counts,
    };
    let audit = audit_overlay(
        &selection,
        &sha256_hex(&selection_bytes),
        &overlay,
        &source_context,
    )?;

    Ok(TranslationOverlayAuditReport {
        overlay: overlay_path.to_path_buf(),
        overlay_sha256: sha256_hex(&overlay_bytes),
        contexts: contexts_path.to_path_buf(),
        contexts_sha256,
        context_assignment_count: context_assignments.len(),
        selection: selection_path.to_path_buf(),
        selection_sha256: sha256_hex(&selection_bytes),
        message_catalog_sha256,
        inline_text_catalog_sha256,
        entry_count: overlay.entries.len(),
        translated_entry_count: audit.translated_entry_count,
        preserve_source_entry_count: audit.preserve_source_entry_count,
        translated_segment_count: audit.translated_segment_count,
        translated_character_count: audit.translated_character_count,
        unique_non_whitespace_character_count: audit.unique_non_whitespace_characters.len(),
        required_external_character_count: audit.required_external_characters.len(),
        required_external_characters: audit
            .required_external_characters
            .iter()
            .map(char::to_string)
            .collect(),
        static_external_character_capacity: STATIC_EXTERNAL_CHARACTER_CAPACITY,
        fits_static_external_character_capacity: audit.required_external_characters.len()
            <= STATIC_EXTERNAL_CHARACTER_CAPACITY,
        longer_than_source_segment_count: audit.longer_than_source_segments.len(),
        maximum_segment_cell_count: audit.maximum_segment_cell_count,
        maximum_positive_cell_growth: audit.maximum_positive_cell_growth,
        longer_than_source_segments: audit.longer_than_source_segments,
        status: "the source-bound Korean wording selection, context assignments, and per-segment static cell comparison were audited; wording remains human-review pending, and byte encoding, glyph installation, screen layout, presentation, runtime, build eligibility, and distribution eligibility are not proven",
    })
}

pub fn audit_verified_translation_overlay_catalog(
    source_cd_path: &Path,
    system_hdi_path: &Path,
    catalog_path: &Path,
) -> Result<TranslationOverlayCatalogAuditReport> {
    let files = load_verified_game_files(source_cd_path, system_hdi_path)?;
    let message_catalog = catalog_messages(&files)?;
    let inline_text_catalog = catalog_inline_text(&files)?;
    let message_catalog_sha256 = serialized_sha256(&message_catalog)?;
    let inline_text_catalog_sha256 = serialized_sha256(&inline_text_catalog)?;
    let loaded = load_translation_overlay_catalog(
        catalog_path,
        &message_catalog_sha256,
        &message_catalog,
        &inline_text_catalog_sha256,
        &inline_text_catalog,
    )?;

    Ok(TranslationOverlayCatalogAuditReport {
        catalog: loaded.catalog,
        catalog_sha256: loaded.catalog_sha256,
        contexts: loaded.contexts,
        contexts_sha256: loaded.contexts_sha256,
        context_assignment_count: loaded.context_assignment_count,
        message_catalog_sha256,
        inline_text_catalog_sha256,
        batch_count: loaded.batch_count,
        entry_count: loaded.entry_count,
        translated_entry_count: loaded.audit.translated_entry_count,
        preserve_source_entry_count: loaded.audit.preserve_source_entry_count,
        translated_segment_count: loaded.audit.translated_segment_count,
        translated_character_count: loaded.audit.translated_character_count,
        unique_non_whitespace_character_count: loaded.audit.unique_non_whitespace_characters.len(),
        required_external_character_count: loaded.audit.required_external_characters.len(),
        required_external_characters: loaded
            .audit
            .required_external_characters
            .iter()
            .map(char::to_string)
            .collect(),
        static_external_character_capacity: STATIC_EXTERNAL_CHARACTER_CAPACITY,
        fits_static_external_character_capacity: loaded.audit.required_external_characters.len()
            <= STATIC_EXTERNAL_CHARACTER_CAPACITY,
        longer_than_source_segment_count: loaded.audit.longer_than_source_segments.len(),
        maximum_segment_cell_count: loaded.audit.maximum_segment_cell_count,
        maximum_positive_cell_growth: loaded.audit.maximum_positive_cell_growth,
        longer_than_source_segments: loaded.audit.longer_than_source_segments,
        status: "all source-bound Korean translation batches have distinct entry ownership and matching context assignments; wording remains human-review pending, and byte encoding, glyph installation, screen layout, presentation, runtime, build eligibility, and distribution eligibility are not proven",
    })
}

pub(crate) fn load_translation_overlay_catalog(
    catalog_path: &Path,
    message_catalog_sha256: &str,
    message_catalog: &MessageCatalog,
    inline_text_catalog_sha256: &str,
    inline_text_catalog: &InlineTextCatalog,
) -> Result<LoadedTranslationOverlayCatalog> {
    let catalog_bytes = fs::read(catalog_path).with_context(|| {
        format!(
            "could not read translation overlay catalog {}",
            catalog_path.display()
        )
    })?;
    let catalog: TranslationOverlayCatalog =
        serde_json::from_slice(&catalog_bytes).with_context(|| {
            format!(
                "could not parse translation overlay catalog {}",
                catalog_path.display()
            )
        })?;
    ensure!(
        catalog.schema == OVERLAY_CATALOG_SCHEMA,
        "unsupported translation overlay catalog schema"
    );
    ensure!(
        !catalog.batches.is_empty(),
        "translation overlay catalog has no batches"
    );
    audit_source_catalog_bindings(
        &catalog.source_catalogs,
        message_catalog_sha256,
        message_catalog.translation_source_cell_counts().len(),
        inline_text_catalog_sha256,
        inline_text_catalog.translation_source_cell_counts().len(),
    )?;

    let catalog_parent = catalog_path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let contexts_path = resolve_catalog_path(catalog_parent, &catalog.contexts.path, "contexts")?;
    let message_ids = message_catalog
        .translation_layouts()
        .map(|(id, _)| id.to_owned())
        .collect::<Vec<_>>();
    let inline_text_ids = inline_text_catalog
        .translation_layouts()
        .map(|(id, _)| id.to_owned())
        .collect::<Vec<_>>();
    let (contexts_sha256, context_assignments) = audit_translation_context_assignments(
        &contexts_path,
        message_catalog_sha256,
        &message_ids,
        inline_text_catalog_sha256,
        &inline_text_ids,
    )?;
    ensure!(
        contexts_sha256 == catalog.contexts.sha256,
        "translation context audit hash differs from catalog binding"
    );

    let message_source_cell_counts = message_catalog.translation_source_cell_counts();
    let inline_text_source_cell_counts = inline_text_catalog.translation_source_cell_counts();
    let source_context = OverlaySourceContext {
        contexts_sha256: &contexts_sha256,
        context_assignments: &context_assignments,
        message_catalog_sha256,
        message_source_cell_counts: &message_source_cell_counts,
        inline_text_catalog_sha256,
        inline_text_source_cell_counts: &inline_text_source_cell_counts,
    };
    let mut seen_batch_ids = BTreeSet::new();
    let mut seen_entry_ids = BTreeSet::new();
    let mut aggregate = OverlayAudit::default();
    let mut overlays = Vec::with_capacity(catalog.batches.len());
    for batch in &catalog.batches {
        ensure!(
            !batch.id.trim().is_empty() && seen_batch_ids.insert(batch.id.as_str()),
            "translation overlay catalog has an empty or duplicate batch id"
        );
        let selection_path =
            resolve_catalog_path(catalog_parent, &batch.selection, "batch selection")?;
        let overlay_path = resolve_catalog_path(catalog_parent, &batch.overlay, "batch overlay")?;
        let (selection, selection_bytes) = read_selection(&selection_path)?;
        let (selection_id, _) = validate_selection(&selection)?;
        ensure!(
            selection_id == batch.id && sha256_hex(&selection_bytes) == batch.selection_sha256,
            "translation batch {} selection identity or hash changed",
            batch.id
        );
        let (overlay, overlay_bytes) = read_overlay(&overlay_path)?;
        ensure!(
            sha256_hex(&overlay_bytes) == batch.overlay_sha256
                && overlay.entries.len() == batch.entry_count,
            "translation batch {} overlay hash or population changed",
            batch.id
        );
        let audit = audit_overlay(
            &selection,
            &batch.selection_sha256,
            &overlay,
            &source_context,
        )?;
        record_entry_ownership(&mut seen_entry_ids, &overlay)?;
        aggregate.merge(audit);
        overlays.push(overlay);
    }

    Ok(LoadedTranslationOverlayCatalog {
        catalog: catalog_path.to_path_buf(),
        catalog_sha256: sha256_hex(&catalog_bytes),
        entry_count: seen_entry_ids.len(),
        contexts: contexts_path,
        contexts_sha256,
        context_assignment_count: context_assignments.len(),
        batch_count: overlays.len(),
        audit: aggregate,
        overlays,
    })
}

impl LoadedTranslationOverlayCatalog {
    pub(crate) fn required_external_characters(&self) -> &BTreeSet<char> {
        &self.audit.required_external_characters
    }

    pub(crate) fn apply_to_draft(&self, draft: &mut TranslationDraft) -> Result<()> {
        for entry in self.overlays.iter().flat_map(|overlay| &overlay.entries) {
            draft.apply_translation(
                &entry.collection,
                &entry.id,
                &entry.ko_segments,
                &entry.status,
                &entry.notes,
            )?;
        }
        Ok(())
    }
}

fn read_selection(path: &Path) -> Result<(TranslationSelection, Vec<u8>)> {
    let bytes = fs::read(path)
        .with_context(|| format!("could not read translation selection {}", path.display()))?;
    let selection = serde_json::from_slice(&bytes)
        .with_context(|| format!("could not parse translation selection {}", path.display()))?;
    Ok((selection, bytes))
}

fn read_overlay(path: &Path) -> Result<(TranslationOverlay, Vec<u8>)> {
    let bytes = fs::read(path)
        .with_context(|| format!("could not read translation overlay {}", path.display()))?;
    let overlay = serde_json::from_slice(&bytes)
        .with_context(|| format!("could not parse translation overlay {}", path.display()))?;
    Ok((overlay, bytes))
}

fn resolve_catalog_path(parent: &Path, relative: &Path, role: &str) -> Result<PathBuf> {
    ensure!(
        !relative.as_os_str().is_empty() && !relative.is_absolute(),
        "translation overlay catalog {role} path must be nonempty and relative"
    );
    Ok(parent.join(relative))
}

fn record_entry_ownership(
    seen_entry_ids: &mut BTreeSet<String>,
    overlay: &TranslationOverlay,
) -> Result<()> {
    for entry in &overlay.entries {
        ensure!(
            seen_entry_ids.insert(entry.id.clone()),
            "translation entry {} is owned by more than one batch",
            entry.id
        );
    }
    Ok(())
}

fn audit_source_catalog_bindings(
    bindings: &[SourceCatalogBinding],
    message_catalog_sha256: &str,
    message_entry_count: usize,
    inline_text_catalog_sha256: &str,
    inline_text_entry_count: usize,
) -> Result<()> {
    let expected_sources = [
        (
            MESSAGE_COLLECTION,
            message_catalog_sha256,
            message_entry_count,
        ),
        (
            INLINE_TEXT_COLLECTION,
            inline_text_catalog_sha256,
            inline_text_entry_count,
        ),
    ];
    ensure!(
        bindings.len() == expected_sources.len(),
        "translation asset must bind exactly two source catalogs"
    );
    for (binding, (expected_id, expected_sha256, expected_entry_count)) in
        bindings.iter().zip(expected_sources)
    {
        ensure!(
            binding.id == expected_id
                && binding.sha256 == expected_sha256
                && binding.entry_count == expected_entry_count,
            "translation source binding {expected_id} changed"
        );
    }
    Ok(())
}

fn audit_overlay(
    selection: &TranslationSelection,
    selection_sha256: &str,
    overlay: &TranslationOverlay,
    source: &OverlaySourceContext<'_>,
) -> Result<OverlayAudit> {
    let (selection_id, expected_scope) = validate_selection(selection)?;
    ensure!(
        selection.context_catalog_sha256 == source.contexts_sha256,
        "translation selection context catalog binding changed"
    );
    ensure!(
        overlay.schema == OVERLAY_SCHEMA,
        "unsupported translation overlay schema"
    );
    ensure!(
        overlay.scope == expected_scope,
        "translation overlay scope does not match its selection"
    );
    audit_source_catalog_bindings(
        &overlay.source_catalogs,
        source.message_catalog_sha256,
        source.message_source_cell_counts.len(),
        source.inline_text_catalog_sha256,
        source.inline_text_source_cell_counts.len(),
    )?;
    ensure!(
        overlay.selection.id == selection_id
            && overlay.selection.sha256 == selection_sha256
            && overlay.selection.entry_count == selection.entries.len(),
        "translation overlay selection binding changed"
    );
    ensure!(
        overlay.entries.len() == selection.entries.len(),
        "translation overlay population differs from its selection"
    );

    let mut seen_ids = BTreeSet::new();
    let mut translated_entry_count = 0usize;
    let mut preserve_source_entry_count = 0usize;
    let mut translated_segment_count = 0usize;
    let mut translated_character_count = 0usize;
    let mut unique_non_whitespace_characters = BTreeSet::new();
    let mut required_external_characters = BTreeSet::new();
    let mut longer_than_source_segments = Vec::new();
    let mut maximum_segment_cell_count = 0usize;
    let mut maximum_positive_cell_growth = 0usize;

    for (selected, entry) in selection.entries.iter().zip(&overlay.entries) {
        ensure!(
            entry.id == selected.id && seen_ids.insert(entry.id.as_str()),
            "translation overlay changed, reordered, or duplicated a selected entry"
        );
        ensure!(
            !selected.context_id.is_empty() && !selected.goals.is_empty(),
            "translation selection entry {} lacks review context",
            selected.id
        );
        ensure!(
            source.context_assignments.get(&selected.id) == Some(&selected.context_id),
            "translation selection entry {} has an incorrect context assignment",
            selected.id
        );
        let source_cell_counts = match entry.collection.as_str() {
            MESSAGE_COLLECTION => source.message_source_cell_counts.get(&entry.id),
            INLINE_TEXT_COLLECTION => source.inline_text_source_cell_counts.get(&entry.id),
            _ => None,
        }
        .with_context(|| {
            format!(
                "translation overlay entry {} is absent from collection {}",
                entry.id, entry.collection
            )
        })?;
        ensure!(
            entry.ko_segments.len() == source_cell_counts.len(),
            "translation overlay entry {} changed its source-owned segment count",
            entry.id
        );
        validate_translation_fields(
            &entry.collection,
            &entry.id,
            &entry.ko_segments,
            &entry.status,
            &entry.notes,
        )?;
        if entry.status == PRESERVE_SOURCE_STATUS {
            ensure!(
                selected
                    .goals
                    .iter()
                    .any(|goal| goal == "source_preservation"),
                "source-preserved overlay entry {} was not selected for source preservation",
                entry.id
            );
            preserve_source_entry_count += 1;
        } else {
            translated_entry_count += 1;
        }

        for (segment_index, (segment, source_cell_count)) in
            entry.ko_segments.iter().zip(source_cell_counts).enumerate()
        {
            if segment.is_empty() {
                continue;
            }
            let runtime_characters = if entry.collection == MESSAGE_COLLECTION {
                crate::josa::manual_message_lines(segment)
                    .with_context(|| {
                        format!(
                            "translation overlay entry {} segment {} has invalid manual line layout",
                            entry.id, segment_index
                        )
                    })?
                    .into_iter()
                    .map(crate::josa::runtime_text_characters)
                    .collect::<Result<Vec<_>>>()?
                    .into_iter()
                    .flatten()
                    .collect::<Vec<_>>()
            } else {
                crate::josa::runtime_text_characters(segment).with_context(|| {
                    format!(
                        "translation overlay entry {} segment {} has invalid particle syntax",
                        entry.id, segment_index
                    )
                })?
            };
            ensure!(
                !runtime_characters.iter().copied().any(is_japanese_script),
                "translation overlay entry {} segment {} still contains Japanese script",
                entry.id,
                segment_index
            );
            translated_segment_count += 1;
            let translated_cell_count = runtime_characters.len();
            translated_character_count += translated_cell_count;
            maximum_segment_cell_count = maximum_segment_cell_count.max(translated_cell_count);
            if translated_cell_count > *source_cell_count {
                maximum_positive_cell_growth =
                    maximum_positive_cell_growth.max(translated_cell_count - source_cell_count);
                longer_than_source_segments.push(format!("{}#{}", entry.id, segment_index));
            }
            for character in runtime_characters {
                if !character.is_whitespace() {
                    unique_non_whitespace_characters.insert(character);
                }
                let text = character.to_string();
                let (_, _, had_errors) = SHIFT_JIS.encode(&text);
                if had_errors {
                    required_external_characters.insert(character);
                }
            }
        }
    }

    Ok(OverlayAudit {
        translated_entry_count,
        preserve_source_entry_count,
        translated_segment_count,
        translated_character_count,
        unique_non_whitespace_characters,
        required_external_characters,
        longer_than_source_segments,
        maximum_segment_cell_count,
        maximum_positive_cell_growth,
    })
}

impl OverlayAudit {
    fn merge(&mut self, other: Self) {
        self.translated_entry_count += other.translated_entry_count;
        self.preserve_source_entry_count += other.preserve_source_entry_count;
        self.translated_segment_count += other.translated_segment_count;
        self.translated_character_count += other.translated_character_count;
        self.unique_non_whitespace_characters
            .extend(other.unique_non_whitespace_characters);
        self.required_external_characters
            .extend(other.required_external_characters);
        self.longer_than_source_segments
            .extend(other.longer_than_source_segments);
        self.maximum_segment_cell_count = self
            .maximum_segment_cell_count
            .max(other.maximum_segment_cell_count);
        self.maximum_positive_cell_growth = self
            .maximum_positive_cell_growth
            .max(other.maximum_positive_cell_growth);
    }
}

fn validate_selection(selection: &TranslationSelection) -> Result<(&str, &str)> {
    ensure!(
        !selection.context_catalog_sha256.is_empty(),
        "translation selection lacks a context catalog binding"
    );
    match selection.schema.as_str() {
        REPRESENTATIVE_SELECTION_SCHEMA => {
            ensure!(
                selection.selection_status.as_deref() == Some("selected_for_first_draft")
                    && selection.batch_id.is_none(),
                "representative translation selection has invalid state fields"
            );
            Ok(("representative_sample", REPRESENTATIVE_SCOPE))
        }
        BATCH_SELECTION_SCHEMA => {
            let batch_id = selection
                .batch_id
                .as_deref()
                .filter(|batch_id| !batch_id.trim().is_empty())
                .context("translation batch selection has no batch id")?;
            ensure!(
                selection.selection_status.is_none(),
                "translation batch selection must not carry representative state"
            );
            Ok((batch_id, CONTEXT_BATCH_SCOPE))
        }
        _ => bail!("unsupported translation selection schema"),
    }
}

fn is_japanese_script(character: char) -> bool {
    matches!(
        u32::from(character),
        0x3040..=0x30ff | 0x31f0..=0x31ff | 0xff65..=0xff9f
    )
}

#[cfg(test)]
#[path = "translation_overlay_tests.rs"]
mod tests;
