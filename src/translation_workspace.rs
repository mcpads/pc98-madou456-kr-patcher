use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, ensure};
use serde::Serialize;
use tempfile::Builder;

use crate::graphic_export::{GraphicAtlasFileReport, export_masked_tile_atlases_from_files};
use crate::reassembly::{SourceReport, load_verified_game_files_with_report};

use super::graphic_text_review::{GraphicTextCandidate, translation_graphic_candidates};
use super::translation_draft::build_translation_draft;
use super::{catalog_inline_text, catalog_messages, write_pretty_json_noclobber};

const MESSAGE_CATALOG_FILE: &str = "messages.json";
const INLINE_TEXT_CATALOG_FILE: &str = "inline-text.json";
const GRAPHIC_DIRECTORY: &str = "graphic-text";
const TRANSLATION_DRAFT_FILE: &str = "translation-draft.json";
const MANIFEST_FILE: &str = "manifest.json";
const PROGRAM_LINKED_STATUS: &str = "program-linked";
const UNRESOLVED_STATUS: &str = "unresolved";

#[derive(Debug, Serialize)]
pub struct TranslationWorkspaceWriteReport {
    output_directory: PathBuf,
    manifest_sha256: String,
    message_catalog_sha256: String,
    message_entry_count: usize,
    inline_text_catalog_sha256: String,
    inline_text_entry_count: usize,
    translation_draft_sha256: String,
    translation_draft_entry_count: usize,
    graphic_candidate_count: usize,
    program_linked_graphic_count: usize,
    unresolved_graphic_count: usize,
}

#[derive(Debug, Serialize)]
struct TranslationWorkspaceManifest {
    schema: &'static str,
    sources: SourceReport,
    scope: TranslationScope,
    catalogs: Vec<TranslationCatalog>,
    graphics: TranslationGraphics,
}

#[derive(Debug, Serialize)]
struct TranslationScope {
    included: &'static str,
    excluded: [&'static str; 3],
    privacy: &'static str,
}

#[derive(Debug, Serialize)]
struct TranslationCatalog {
    id: &'static str,
    file: &'static str,
    sha256: String,
    entry_count: usize,
    contains_original_game_text: bool,
}

#[derive(Debug, Serialize)]
struct TranslationGraphics {
    directory: &'static str,
    render_kind: &'static str,
    candidate_count: usize,
    program_linked_count: usize,
    unresolved_count: usize,
    candidates: Vec<GraphicTextCandidate>,
    rendered_files: Vec<GraphicAtlasFileReport>,
}

pub fn write_verified_translation_workspace(
    source_cd_path: &Path,
    system_hdi_path: &Path,
    output_directory: &Path,
) -> Result<TranslationWorkspaceWriteReport> {
    ensure!(
        !output_directory.exists(),
        "refusing to overwrite existing translation workspace {}",
        output_directory.display()
    );

    let (sources, files) = load_verified_game_files_with_report(source_cd_path, system_hdi_path)?;
    let message_catalog = catalog_messages(&files)?;
    let inline_text_catalog = catalog_inline_text(&files)?;
    let mut graphics = translation_graphics()?;

    let parent = output_directory
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent).with_context(|| {
        format!(
            "could not create translation workspace parent {}",
            parent.display()
        )
    })?;
    let staging = Builder::new()
        .prefix(".madou456-translation-workspace-")
        .tempdir_in(parent)
        .with_context(|| {
            format!(
                "could not create translation workspace staging directory in {}",
                parent.display()
            )
        })?;

    let message_catalog_sha256 = write_pretty_json_noclobber(
        &message_catalog,
        &staging.path().join(MESSAGE_CATALOG_FILE),
        "translation workspace message catalog",
    )?;
    let inline_text_catalog_sha256 = write_pretty_json_noclobber(
        &inline_text_catalog,
        &staging.path().join(INLINE_TEXT_CATALOG_FILE),
        "translation workspace inline text catalog",
    )?;
    let translation_draft = build_translation_draft(
        &message_catalog_sha256,
        message_catalog.translation_layouts(),
        &inline_text_catalog_sha256,
        inline_text_catalog.translation_layouts(),
    )?;
    let translation_draft_entry_count = translation_draft.entry_count();
    let translation_draft_sha256 = write_pretty_json_noclobber(
        &translation_draft,
        &staging.path().join(TRANSLATION_DRAFT_FILE),
        "translation workspace draft",
    )?;
    let graphic_names = graphics
        .candidates
        .iter()
        .map(|candidate| candidate.name.to_owned())
        .collect::<Vec<_>>();
    let graphic_report = export_masked_tile_atlases_from_files(
        &files,
        &graphic_names,
        &staging.path().join(GRAPHIC_DIRECTORY),
    )?;
    ensure!(
        graphic_report.atlas_count() == graphics.candidate_count,
        "translation graphic render population changed"
    );
    graphics.rendered_files = graphic_report.into_files();

    let message_entry_count = message_catalog.message_entry_count;
    let inline_text_entry_count = inline_text_catalog.analysis.entry_count;
    let graphic_candidate_count = graphics.candidate_count;
    let program_linked_graphic_count = graphics.program_linked_count;
    let unresolved_graphic_count = graphics.unresolved_count;
    let manifest = TranslationWorkspaceManifest {
        schema: "pc98_madou456.translation_workspace",
        sources,
        scope: TranslationScope {
            included: "verified game messages, executable-owned opening/ending text, and reviewed game graphic-text candidates",
            excluded: [
                "installed DOS and command-interpreter messages",
                "DORI-BIOS and support-driver diagnostics",
                "PC-98 built-in BASIC messages",
            ],
            privacy: "contains original game text and pixels; keep the entire workspace under ignored out/ and do not publish or commit it",
        },
        catalogs: vec![
            TranslationCatalog {
                id: "messages",
                file: MESSAGE_CATALOG_FILE,
                sha256: message_catalog_sha256.clone(),
                entry_count: message_entry_count,
                contains_original_game_text: true,
            },
            TranslationCatalog {
                id: "inline_text",
                file: INLINE_TEXT_CATALOG_FILE,
                sha256: inline_text_catalog_sha256.clone(),
                entry_count: inline_text_entry_count,
                contains_original_game_text: true,
            },
            TranslationCatalog {
                id: "translation_draft",
                file: TRANSLATION_DRAFT_FILE,
                sha256: translation_draft_sha256.clone(),
                entry_count: translation_draft_entry_count,
                contains_original_game_text: false,
            },
        ],
        graphics,
    };
    let manifest_sha256 = write_pretty_json_noclobber(
        &manifest,
        &staging.path().join(MANIFEST_FILE),
        "translation workspace manifest",
    )?;

    let staging_path = staging.keep();
    fs::rename(&staging_path, output_directory).with_context(|| {
        format!(
            "could not publish translation workspace {}",
            output_directory.display()
        )
    })?;

    Ok(TranslationWorkspaceWriteReport {
        output_directory: output_directory.to_path_buf(),
        manifest_sha256,
        message_catalog_sha256,
        message_entry_count,
        inline_text_catalog_sha256,
        inline_text_entry_count,
        translation_draft_sha256,
        translation_draft_entry_count,
        graphic_candidate_count,
        program_linked_graphic_count,
        unresolved_graphic_count,
    })
}

fn translation_graphics() -> Result<TranslationGraphics> {
    let candidates = translation_graphic_candidates();
    let program_linked_count = candidates
        .iter()
        .filter(|candidate| candidate.consumer_status == PROGRAM_LINKED_STATUS)
        .count();
    let unresolved_count = candidates
        .iter()
        .filter(|candidate| candidate.consumer_status == UNRESOLVED_STATUS)
        .count();
    ensure!(
        program_linked_count + unresolved_count == candidates.len(),
        "translation graphic candidate has an unsupported consumer status"
    );
    Ok(TranslationGraphics {
        directory: GRAPHIC_DIRECTORY,
        render_kind: "private 256x256 diagnostic color and mask PPMs; diagnostic colors are not the runtime palette",
        candidate_count: candidates.len(),
        program_linked_count,
        unresolved_count,
        candidates,
        rendered_files: Vec::new(),
    })
}

#[cfg(test)]
#[path = "translation_workspace_tests.rs"]
mod tests;
