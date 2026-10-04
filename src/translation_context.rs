use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::ops::RangeInclusive;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};

use crate::reassembly::load_verified_game_files;

use super::{catalog_inline_text, catalog_messages, sha256_hex};

const CONTEXT_SCHEMA: &str = "pc98_madou456.translation_contexts";
const SAMPLE_SCHEMA: &str = "pc98_madou456.translation_representative_sample";
const MESSAGE_COLLECTION: &str = "messages";
const INLINE_TEXT_COLLECTION: &str = "inline_text";

const CONTEXT_BASES: [&str; 2] = ["content_structure_inferred", "consumer_role_confirmed"];
const SAMPLE_REQUIREMENTS: [&str; 2] = ["required", "optional"];
const SURFACES: [&str; 7] = [
    "common_ui",
    "event",
    "character_voice",
    "special_dialogue",
    "opening",
    "ending",
    "credits",
];
const REVIEW_FOCI: [&str; 10] = [
    "meaning",
    "voice",
    "terminology",
    "control_boundaries",
    "dynamic_insertion",
    "layout",
    "source_preservation",
    "english_ui",
    "nonstandard_source_bytes",
    "presentation",
];
const SAMPLE_GOALS: [&str; 9] = [
    "meaning",
    "voice",
    "terminology",
    "control_boundaries",
    "dynamic_insertion",
    "layout",
    "source_preservation",
    "english_ui",
    "nonstandard_source_bytes",
];

#[derive(Debug, Serialize)]
pub struct TranslationContextAuditReport {
    contexts: PathBuf,
    contexts_sha256: String,
    representative_sample: PathBuf,
    representative_sample_sha256: String,
    message_catalog_sha256: String,
    message_entry_count: usize,
    inline_text_catalog_sha256: String,
    inline_text_entry_count: usize,
    context_count: usize,
    required_context_count: usize,
    mapped_entry_count: usize,
    representative_sample_entry_count: usize,
    representative_message_count: usize,
    representative_inline_text_count: usize,
    status: &'static str,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TranslationContextCatalog {
    schema: String,
    source_catalogs: Vec<SourceCatalogBinding>,
    contexts: Vec<TranslationContext>,
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
struct TranslationContext {
    id: String,
    surface: String,
    label: String,
    #[serde(default)]
    speaker: Option<String>,
    context_basis: String,
    sample_requirement: String,
    review_focus: Vec<String>,
    selectors: Vec<ContextSelector>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ContextSelector {
    collection: String,
    resource: String,
    ranges: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RepresentativeSample {
    schema: String,
    context_catalog_sha256: String,
    selection_status: String,
    entries: Vec<RepresentativeSampleEntry>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RepresentativeSampleEntry {
    id: String,
    context_id: String,
    goals: Vec<String>,
}

pub fn audit_verified_translation_contexts(
    source_cd_path: &Path,
    system_hdi_path: &Path,
    contexts_path: &Path,
    representative_sample_path: &Path,
) -> Result<TranslationContextAuditReport> {
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

    let contexts_bytes = read_json_asset(contexts_path, "translation context catalog")?;
    let contexts: TranslationContextCatalog = serde_json::from_slice(&contexts_bytes)
        .with_context(|| {
            format!(
                "could not parse translation context catalog {}",
                contexts_path.display()
            )
        })?;
    let contexts_sha256 = sha256_hex(&contexts_bytes);
    let assignments = audit_context_catalog(
        &contexts,
        &message_catalog_sha256,
        &message_ids,
        &inline_text_catalog_sha256,
        &inline_text_ids,
    )?;

    let sample_bytes = read_json_asset(representative_sample_path, "representative sample")?;
    let sample: RepresentativeSample =
        serde_json::from_slice(&sample_bytes).with_context(|| {
            format!(
                "could not parse representative sample {}",
                representative_sample_path.display()
            )
        })?;
    audit_representative_sample(&sample, &contexts_sha256, &contexts, &assignments)?;

    let representative_message_count = sample
        .entries
        .iter()
        .filter(|entry| message_ids.iter().any(|id| id == &entry.id))
        .count();
    let representative_inline_text_count = sample.entries.len() - representative_message_count;
    Ok(TranslationContextAuditReport {
        contexts: contexts_path.to_path_buf(),
        contexts_sha256,
        representative_sample: representative_sample_path.to_path_buf(),
        representative_sample_sha256: sha256_hex(&sample_bytes),
        message_catalog_sha256,
        message_entry_count: message_ids.len(),
        inline_text_catalog_sha256,
        inline_text_entry_count: inline_text_ids.len(),
        context_count: contexts.contexts.len(),
        required_context_count: contexts
            .contexts
            .iter()
            .filter(|context| context.sample_requirement == "required")
            .count(),
        mapped_entry_count: assignments.len(),
        representative_sample_entry_count: sample.entries.len(),
        representative_message_count,
        representative_inline_text_count,
        status: "every extracted text entry is assigned to exactly one source-bound review context and the representative first-draft sample covers every required context; speaker and scene labels based on content remain static inferences, and translation quality, layout fit, glyph coverage, presentation, runtime, build eligibility, and distribution eligibility are not proven",
    })
}

pub(super) fn audit_translation_context_assignments(
    contexts_path: &Path,
    message_catalog_sha256: &str,
    message_ids: &[String],
    inline_text_catalog_sha256: &str,
    inline_text_ids: &[String],
) -> Result<(String, BTreeMap<String, String>)> {
    let contexts_bytes = read_json_asset(contexts_path, "translation context catalog")?;
    let contexts: TranslationContextCatalog = serde_json::from_slice(&contexts_bytes)
        .with_context(|| {
            format!(
                "could not parse translation context catalog {}",
                contexts_path.display()
            )
        })?;
    let assignments = audit_context_catalog(
        &contexts,
        message_catalog_sha256,
        message_ids,
        inline_text_catalog_sha256,
        inline_text_ids,
    )?;
    Ok((sha256_hex(&contexts_bytes), assignments))
}

fn audit_context_catalog(
    catalog: &TranslationContextCatalog,
    message_catalog_sha256: &str,
    message_ids: &[String],
    inline_text_catalog_sha256: &str,
    inline_text_ids: &[String],
) -> Result<BTreeMap<String, String>> {
    ensure!(
        catalog.schema == CONTEXT_SCHEMA,
        "unsupported translation context schema"
    );
    let expected_bindings = [
        (
            MESSAGE_COLLECTION,
            message_catalog_sha256,
            message_ids.len(),
        ),
        (
            INLINE_TEXT_COLLECTION,
            inline_text_catalog_sha256,
            inline_text_ids.len(),
        ),
    ];
    ensure!(
        catalog.source_catalogs.len() == expected_bindings.len(),
        "translation contexts must bind exactly the message and inline-text catalogs"
    );
    for (binding, (expected_id, expected_sha256, expected_count)) in
        catalog.source_catalogs.iter().zip(expected_bindings)
    {
        ensure!(
            binding.id == expected_id
                && binding.sha256 == expected_sha256
                && binding.entry_count == expected_count,
            "translation context source binding {expected_id} changed"
        );
    }

    let all_entries = message_ids
        .iter()
        .map(|id| (MESSAGE_COLLECTION, id.as_str()))
        .chain(
            inline_text_ids
                .iter()
                .map(|id| (INLINE_TEXT_COLLECTION, id.as_str())),
        )
        .collect::<Vec<_>>();
    let expected_ids = all_entries
        .iter()
        .map(|(_, id)| *id)
        .collect::<BTreeSet<_>>();
    let mut context_ids = BTreeSet::new();
    let mut assignments = BTreeMap::new();
    for context in &catalog.contexts {
        validate_context_shape(context, &mut context_ids)?;
        let mut context_entry_count = 0usize;
        for selector in &context.selectors {
            let ranges = parse_selector_ranges(selector)?;
            let mut selector_entry_count = 0usize;
            for (collection, id) in &all_entries {
                if selector_matches(collection, id, selector, &ranges)? {
                    ensure!(
                        assignments
                            .insert((*id).to_owned(), context.id.clone())
                            .is_none(),
                        "translation entry {id} is assigned to more than one context"
                    );
                    selector_entry_count += 1;
                }
            }
            ensure!(
                selector_entry_count > 0,
                "context {} has a selector that matches no source entry",
                context.id
            );
            context_entry_count += selector_entry_count;
        }
        ensure!(
            context_entry_count > 0,
            "translation context {} is empty",
            context.id
        );
    }

    let actual_ids = assignments
        .keys()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    ensure!(
        actual_ids == expected_ids,
        "translation contexts do not cover the source entry population exactly"
    );
    Ok(assignments)
}

fn validate_context_shape<'a>(
    context: &'a TranslationContext,
    context_ids: &mut BTreeSet<&'a str>,
) -> Result<()> {
    ensure!(
        !context.id.trim().is_empty() && context_ids.insert(context.id.as_str()),
        "translation context has an empty or duplicated id"
    );
    ensure!(
        SURFACES.contains(&context.surface.as_str())
            && !context.label.trim().is_empty()
            && context
                .speaker
                .as_ref()
                .is_none_or(|speaker| !speaker.trim().is_empty())
            && CONTEXT_BASES.contains(&context.context_basis.as_str())
            && SAMPLE_REQUIREMENTS.contains(&context.sample_requirement.as_str())
            && !context.selectors.is_empty(),
        "translation context {} has an unsupported surface, basis, sample requirement, or empty label/selectors",
        context.id
    );
    let mut focus = BTreeSet::new();
    ensure!(
        !context.review_focus.is_empty()
            && context.review_focus.iter().all(|item| {
                REVIEW_FOCI.contains(&item.as_str()) && focus.insert(item.as_str())
            }),
        "translation context {} has an empty, duplicated, or unsupported review focus",
        context.id
    );
    Ok(())
}

fn parse_selector_ranges(selector: &ContextSelector) -> Result<Vec<RangeInclusive<usize>>> {
    let (width, radix) = match selector.collection.as_str() {
        MESSAGE_COLLECTION => (2, 16),
        INLINE_TEXT_COLLECTION => (3, 10),
        _ => {
            ensure!(false, "context selector has an unsupported collection");
            unreachable!()
        }
    };
    ensure!(
        !selector.resource.trim().is_empty() && !selector.ranges.is_empty(),
        "context selector has an empty resource or range list"
    );
    let mut parsed = Vec::with_capacity(selector.ranges.len());
    let mut previous_end = None;
    for encoded in &selector.ranges {
        let (start_text, end_text) = encoded
            .split_once('-')
            .map_or((encoded.as_str(), encoded.as_str()), |(start, end)| {
                (start, end)
            });
        let start = parse_selector_bound(start_text, width, radix)?;
        let end = parse_selector_bound(end_text, width, radix)?;
        ensure!(start <= end, "context selector range is descending");
        ensure!(
            previous_end.is_none_or(|value| start > value),
            "context selector ranges overlap or are out of order"
        );
        previous_end = Some(end);
        parsed.push(start..=end);
    }
    Ok(parsed)
}

fn parse_selector_bound(encoded: &str, width: usize, radix: u32) -> Result<usize> {
    ensure!(
        encoded.len() == width
            && encoded.chars().all(|character| character.is_ascii_digit()
                || (radix == 16 && ('A'..='F').contains(&character))),
        "context selector bound {encoded} is not canonical"
    );
    usize::from_str_radix(encoded, radix)
        .with_context(|| format!("could not parse context selector bound {encoded}"))
}

fn selector_matches(
    collection: &str,
    id: &str,
    selector: &ContextSelector,
    ranges: &[RangeInclusive<usize>],
) -> Result<bool> {
    if collection != selector.collection {
        return Ok(false);
    }
    let (resource, encoded_index) = id
        .rsplit_once(':')
        .with_context(|| format!("translation entry id {id} has no resource separator"))?;
    if resource != selector.resource {
        return Ok(false);
    }
    let radix = if collection == MESSAGE_COLLECTION {
        16
    } else {
        10
    };
    let index = usize::from_str_radix(encoded_index, radix)
        .with_context(|| format!("translation entry id {id} has an invalid index"))?;
    Ok(ranges.iter().any(|range| range.contains(&index)))
}

fn audit_representative_sample(
    sample: &RepresentativeSample,
    context_catalog_sha256: &str,
    contexts: &TranslationContextCatalog,
    assignments: &BTreeMap<String, String>,
) -> Result<()> {
    ensure!(
        sample.schema == SAMPLE_SCHEMA,
        "unsupported representative sample schema"
    );
    ensure!(
        sample.context_catalog_sha256 == context_catalog_sha256,
        "representative sample is not bound to the current context catalog"
    );
    ensure!(
        sample.selection_status == "selected_for_first_draft",
        "representative sample has an unsupported selection status"
    );
    ensure!(
        (30..=50).contains(&sample.entries.len()),
        "representative sample must contain 30 to 50 entries"
    );

    let mut entry_ids = BTreeSet::new();
    let mut sampled_context_ids = BTreeSet::new();
    let mut sampled_surfaces = BTreeSet::new();
    let context_by_id = contexts
        .contexts
        .iter()
        .map(|context| (context.id.as_str(), context))
        .collect::<BTreeMap<_, _>>();
    for entry in &sample.entries {
        ensure!(
            !entry.id.trim().is_empty() && entry_ids.insert(entry.id.as_str()),
            "representative sample has an empty or duplicated entry id"
        );
        let assigned_context = assignments.get(&entry.id).with_context(|| {
            format!(
                "representative sample entry {} is not in the source population",
                entry.id
            )
        })?;
        ensure!(
            assigned_context == &entry.context_id,
            "representative sample entry {} names the wrong context",
            entry.id
        );
        let context = context_by_id
            .get(entry.context_id.as_str())
            .with_context(|| {
                format!(
                    "representative sample names unknown context {}",
                    entry.context_id
                )
            })?;
        sampled_context_ids.insert(entry.context_id.as_str());
        sampled_surfaces.insert(context.surface.as_str());

        let mut goals = BTreeSet::new();
        ensure!(
            !entry.goals.is_empty()
                && entry.goals.iter().all(|goal| {
                    SAMPLE_GOALS.contains(&goal.as_str()) && goals.insert(goal.as_str())
                }),
            "representative sample entry {} has an empty, duplicated, or unsupported goal",
            entry.id
        );
    }

    ensure!(
        contexts
            .contexts
            .iter()
            .filter(|context| context.sample_requirement == "required")
            .all(|context| sampled_context_ids.contains(context.id.as_str())),
        "representative sample omits a required translation context"
    );
    ensure!(
        SURFACES
            .into_iter()
            .all(|surface| sampled_surfaces.contains(surface)),
        "representative sample does not cover every translation surface"
    );
    Ok(())
}

fn serialized_sha256<T: Serialize>(value: &T) -> Result<String> {
    let mut encoded = serde_json::to_vec_pretty(value)?;
    encoded.push(b'\n');
    Ok(sha256_hex(&encoded))
}

fn read_json_asset(path: &Path, role: &str) -> Result<Vec<u8>> {
    fs::read(path).with_context(|| format!("could not read {role} {}", path.display()))
}

#[cfg(test)]
#[path = "translation_context_tests.rs"]
mod tests;
