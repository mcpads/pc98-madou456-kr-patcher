use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const POLICY_SCHEMA: &str = "pc98_madou456.translation_policy";
const TERMINOLOGY_SCHEMA: &str = "pc98_madou456.translation_terminology";

const WORDING_STATES: [(&str, bool); 6] = [
    ("untranslated", false),
    ("preserve_source", false),
    ("in_progress", true),
    ("needs_review", true),
    ("needs_human_review", true),
    ("approved", true),
];

const QUALITY_GATES: [&str; 8] = [
    "source_binding",
    "wording",
    "layout",
    "glyph_coverage",
    "presentation",
    "runtime",
    "development_build_eligibility",
    "distribution_eligibility",
];

#[derive(Debug, Serialize)]
pub struct TranslationAssetAuditReport {
    policy: PathBuf,
    policy_sha256: String,
    terminology: PathBuf,
    terminology_sha256: String,
    wording_status_count: usize,
    terminology_entry_count: usize,
    reference_project_count: usize,
    status: &'static str,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TranslationPolicy {
    schema: String,
    project_title_ko: String,
    scope: TranslationScope,
    language: LanguagePolicy,
    sibling_reuse: SiblingReusePolicy,
    workflow: TranslationWorkflow,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TranslationScope {
    included: Vec<String>,
    excluded: Vec<String>,
    deferred: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct LanguagePolicy {
    translation_goal: String,
    series_terms: String,
    arle_voice: String,
    other_speakers: String,
    english_ui: EnglishUiPolicy,
    credits: CreditPolicy,
    controls: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct EnglishUiPolicy {
    default: String,
    preserve_when: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CreditPolicy {
    personal_names: String,
    company_names: String,
    role_labels: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SiblingReusePolicy {
    allowed: Vec<String>,
    required_target_checks: Vec<String>,
    not_inherited: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TranslationWorkflow {
    entry_status_dimension: String,
    entry_statuses: Vec<WordingState>,
    separate_gates: Vec<String>,
    development_build: String,
    distribution: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WordingState {
    id: String,
    requires_korean_text: bool,
    meaning: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TranslationTerminology {
    schema: String,
    decision_state: String,
    reference_projects: Vec<ReferenceProject>,
    entries: Vec<TerminologyEntry>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReferenceProject {
    id: String,
    #[serde(rename = "use")]
    usage: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TerminologyEntry {
    id: String,
    source_term: String,
    ko: String,
    target_occurrence_count: usize,
    evidence: String,
}

pub fn audit_translation_assets(
    policy_path: &Path,
    terminology_path: &Path,
) -> Result<TranslationAssetAuditReport> {
    let policy_bytes = read_asset(policy_path, "translation policy")?;
    let policy: TranslationPolicy = serde_json::from_slice(&policy_bytes).with_context(|| {
        format!(
            "could not parse translation policy {}",
            policy_path.display()
        )
    })?;
    validate_translation_policy(&policy)?;

    let terminology_bytes = read_asset(terminology_path, "translation terminology")?;
    let terminology: TranslationTerminology = serde_json::from_slice(&terminology_bytes)
        .with_context(|| {
            format!(
                "could not parse translation terminology {}",
                terminology_path.display()
            )
        })?;
    validate_translation_terminology(&terminology)?;

    Ok(TranslationAssetAuditReport {
        policy: policy_path.to_path_buf(),
        policy_sha256: sha256_hex(&policy_bytes),
        terminology: terminology_path.to_path_buf(),
        terminology_sha256: sha256_hex(&terminology_bytes),
        wording_status_count: policy.workflow.entry_statuses.len(),
        terminology_entry_count: terminology.entries.len(),
        reference_project_count: terminology.reference_projects.len(),
        status: "translation scope, wording-state semantics, separate quality gates, sibling-reuse limits, and source-term glossary structure verified; wording, layout, glyph coverage, presentation, runtime, build eligibility, and distribution eligibility are not proven",
    })
}

fn validate_translation_policy(policy: &TranslationPolicy) -> Result<()> {
    ensure!(
        policy.schema == POLICY_SCHEMA,
        "unsupported translation policy schema"
    );
    ensure!(
        !policy.project_title_ko.trim().is_empty(),
        "translation policy has an empty Korean project title"
    );
    validate_scope(&policy.scope)?;
    validate_language_policy(&policy.language)?;
    validate_sibling_reuse_policy(&policy.sibling_reuse)?;
    validate_translation_workflow(&policy.workflow)
}

fn validate_scope(scope: &TranslationScope) -> Result<()> {
    ensure_unique_nonempty(&scope.included, "included translation scope")?;
    ensure_unique_nonempty(&scope.excluded, "excluded translation scope")?;
    ensure_unique_nonempty(&scope.deferred, "deferred translation scope")?;

    let included = scope
        .included
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    let excluded = scope
        .excluded
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    let deferred = scope
        .deferred
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    ensure!(
        included.contains("game_messages")
            && included.contains("opening_narration_and_character_introductions")
            && included.contains("team_specific_endings")
            && included.contains("player_facing_graphic_text")
            && included.contains("player_facing_english_ui"),
        "translation policy omits a required game-facing surface"
    );
    ensure!(
        excluded.contains("installed_dos_and_command_interpreter_messages")
            && excluded.contains("dori_bios_and_support_driver_diagnostics")
            && excluded.contains("pc98_builtin_basic_messages"),
        "translation policy must exclude the verified non-game system-message families"
    );
    ensure!(
        deferred.contains("graphic_text_without_a_verified_consumer"),
        "translation policy must defer graphic text without a verified consumer"
    );
    ensure!(
        included.is_disjoint(&excluded)
            && included.is_disjoint(&deferred)
            && excluded.is_disjoint(&deferred),
        "translation scope categories overlap"
    );
    Ok(())
}

fn validate_language_policy(language: &LanguagePolicy) -> Result<()> {
    for (name, value) in [
        ("translation goal", language.translation_goal.as_str()),
        ("series-term policy", language.series_terms.as_str()),
        ("Arle voice policy", language.arle_voice.as_str()),
        ("other-speaker policy", language.other_speakers.as_str()),
        ("control-token policy", language.controls.as_str()),
    ] {
        ensure!(
            !value.trim().is_empty(),
            "translation policy has an empty {name}"
        );
    }
    ensure!(
        language.english_ui.default == "translate",
        "player-facing English UI must default to translation"
    );
    ensure_unique_nonempty(
        &language.english_ui.preserve_when,
        "English preservation reasons",
    )?;
    ensure!(
        language.credits.personal_names == "preserve"
            && language.credits.company_names == "preserve"
            && language.credits.role_labels == "translate",
        "credit policy must preserve names and translate role labels"
    );
    Ok(())
}

fn validate_sibling_reuse_policy(policy: &SiblingReusePolicy) -> Result<()> {
    ensure_unique_nonempty(&policy.allowed, "allowed sibling reuse")?;
    ensure_unique_nonempty(&policy.required_target_checks, "sibling target checks")?;
    ensure_unique_nonempty(&policy.not_inherited, "non-inherited sibling evidence")?;

    let checks = policy
        .required_target_checks
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    ensure!(
        [
            "same_referent",
            "compatible_context",
            "target_consumer",
            "target_layout"
        ]
        .into_iter()
        .all(|required| checks.contains(required)),
        "sibling reuse is missing a target-local applicability check"
    );

    let not_inherited = policy
        .not_inherited
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    ensure!(
        [
            "scope",
            "english_policy",
            "dos_message_policy",
            "runtime_evidence",
            "distribution_eligibility",
        ]
        .into_iter()
        .all(|item| not_inherited.contains(item)),
        "sibling reuse must keep target scope and evidence independent"
    );
    Ok(())
}

fn validate_translation_workflow(workflow: &TranslationWorkflow) -> Result<()> {
    ensure!(
        workflow.entry_status_dimension == "wording",
        "entry status must describe wording only"
    );
    let actual_states = workflow
        .entry_statuses
        .iter()
        .map(|state| (state.id.as_str(), state.requires_korean_text))
        .collect::<BTreeSet<_>>();
    let expected_states = WORDING_STATES.into_iter().collect::<BTreeSet<_>>();
    ensure!(
        actual_states == expected_states && workflow.entry_statuses.len() == WORDING_STATES.len(),
        "translation policy wording states or Korean-text requirements changed"
    );
    ensure!(
        workflow
            .entry_statuses
            .iter()
            .all(|state| !state.meaning.trim().is_empty()),
        "translation policy has an undocumented wording state"
    );

    let actual_gates = workflow
        .separate_gates
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    let expected_gates = QUALITY_GATES.into_iter().collect::<BTreeSet<_>>();
    ensure!(
        actual_gates == expected_gates && workflow.separate_gates.len() == QUALITY_GATES.len(),
        "translation quality gates changed or were combined"
    );
    ensure!(
        !workflow.development_build.trim().is_empty() && !workflow.distribution.trim().is_empty(),
        "translation build or distribution policy is empty"
    );
    Ok(())
}

fn validate_translation_terminology(terminology: &TranslationTerminology) -> Result<()> {
    ensure!(
        terminology.schema == TERMINOLOGY_SCHEMA,
        "unsupported translation terminology schema"
    );
    ensure!(
        terminology.decision_state == "draft_standard",
        "terminology must remain a draft standard until human approval"
    );
    ensure!(
        !terminology.reference_projects.is_empty(),
        "translation terminology has no reference project"
    );

    let mut reference_ids = BTreeSet::new();
    for project in &terminology.reference_projects {
        ensure!(
            !project.id.trim().is_empty()
                && reference_ids.insert(project.id.as_str())
                && !project.usage.trim().is_empty(),
            "translation terminology has an empty or duplicated reference project"
        );
    }

    ensure!(
        !terminology.entries.is_empty(),
        "translation terminology is empty"
    );
    let mut ids = BTreeSet::new();
    let mut source_terms = BTreeSet::new();
    for entry in &terminology.entries {
        ensure!(
            !entry.id.trim().is_empty() && ids.insert(entry.id.as_str()),
            "translation terminology has an empty or duplicated entry id"
        );
        ensure!(
            !entry.source_term.trim().is_empty()
                && source_terms.insert(entry.source_term.as_str())
                && entry.source_term.chars().count() <= 32
                && !entry.source_term.contains(['\0', '\r', '\n']),
            "translation terminology entry {} has a duplicated or non-term source value",
            entry.id
        );
        ensure!(
            !entry.ko.trim().is_empty()
                && !entry.ko.contains(['\0', '\r', '\n'])
                && entry.target_occurrence_count > 0
                && !entry.evidence.trim().is_empty(),
            "translation terminology entry {} lacks a Korean term, target occurrence, or evidence",
            entry.id
        );
    }
    Ok(())
}

fn ensure_unique_nonempty(values: &[String], role: &str) -> Result<()> {
    let mut seen = BTreeSet::new();
    ensure!(
        !values.is_empty()
            && values
                .iter()
                .all(|value| !value.trim().is_empty() && seen.insert(value.as_str())),
        "{role} must be nonempty and contain unique values"
    );
    Ok(())
}

fn read_asset(path: &Path, role: &str) -> Result<Vec<u8>> {
    fs::read(path).with_context(|| format!("could not read {role} {}", path.display()))
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[cfg(test)]
#[path = "translation_assets_tests.rs"]
mod tests;
