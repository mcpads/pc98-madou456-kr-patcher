use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, ensure};
use serde::Serialize;
use tempfile::Builder;

use crate::graphic_localization::{GraphicLocalizationReport, build_graphic_localization};
use crate::mado456_ui::{Mado456UiLocalizationReport, Mado456UiTranslation};
use crate::message_buffers::{MessageBufferLayoutReport, validate_message_buffer_layout};
use crate::reassembly::{BuildReport, build_reassembled_hdi_with_replacements};
use crate::translation_font::{TranslationFontReport, build_translation_font_plan};
use crate::translation_renderer::{
    RendererPatchReport, install_drbios_translation_font,
    install_mado456_localization_runtime_support,
    route_inline_renderer_through_translation_glyph_service,
};

use super::inline_text_analysis::RebuiltInlineProgram;
use super::message_analysis::RebuiltMessageFile;
use super::translation_draft::{
    audit_translation_draft, build_translation_draft, serialized_sha256,
};
use super::translation_overlay::load_translation_overlay_catalog;
use super::{
    catalog_inline_text, catalog_messages, load_verified_game_files, require_file, sha256_hex,
};

const MANIFEST_FILE: &str = "manifest.json";
const MESSAGE_COLLECTION: &str = "messages";
const INLINE_TEXT_COLLECTION: &str = "inline_text";
const EXPECTED_LOCALIZED_GAME_FILE_COUNT: usize = 24;

#[derive(Debug, Serialize)]
pub struct LocalizedGameFilesWriteReport {
    output_directory: PathBuf,
    manifest_sha256: String,
    translation_catalog: PathBuf,
    translation_catalog_sha256: String,
    translation_entry_count: usize,
    external_character_count: usize,
    preserved_translation_font_code_count: usize,
    file_count: usize,
    changed_entry_count: usize,
    graphic_translation_unit_count: usize,
    graphic_preserve_source_unit_count: usize,
    status: &'static str,
}

#[derive(Debug, Serialize)]
pub struct LocalizedHdiBuildReport {
    reassembly: BuildReport,
    localized_game_files_manifest_sha256: String,
    translation_catalog: PathBuf,
    translation_catalog_sha256: String,
    translation_entry_count: usize,
    external_character_count: usize,
    preserved_translation_font_code_count: usize,
    replacement_file_count: usize,
    changed_entry_count: usize,
    graphic_translation_unit_count: usize,
    graphic_preserve_source_unit_count: usize,
    status: &'static str,
}

#[derive(Debug, Serialize)]
struct LocalizedGameFilesManifest {
    schema: &'static str,
    translation_catalog: InputAsset,
    source_catalogs: [SourceCatalogAsset; 2],
    font: TranslationFontReport,
    message_buffers: MessageBufferLayoutReport,
    mado456_ui: Mado456UiLocalizationReport,
    graphic_text: GraphicLocalizationReport,
    preserved_translation_font_codes: Vec<String>,
    renderer_patches: Vec<RendererPatchReport>,
    files: Vec<LocalizedGameFileReport>,
    limitations: [&'static str; 4],
}

#[derive(Debug, Serialize)]
struct InputAsset {
    path: PathBuf,
    sha256: String,
    entry_count: usize,
}

#[derive(Debug, Serialize)]
struct SourceCatalogAsset {
    id: &'static str,
    sha256: String,
    entry_count: usize,
}

#[derive(Debug, Serialize)]
struct LocalizedGameFileReport {
    name: String,
    role: &'static str,
    source_size: usize,
    source_sha256: String,
    output_size: usize,
    output_sha256: String,
    changed_entry_count: usize,
    fixed_expected_write_count: usize,
    appended_offset: Option<usize>,
    appended_size: usize,
}

pub fn write_verified_localized_game_files(
    source_cd_path: &Path,
    system_hdi_path: &Path,
    translation_catalog_path: &Path,
    output_directory: &Path,
) -> Result<LocalizedGameFilesWriteReport> {
    ensure!(
        !output_directory.exists(),
        "refusing to overwrite existing localized-game-files directory {}",
        output_directory.display()
    );
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
    audit_translation_draft(
        &draft,
        &message_catalog_sha256,
        message_catalog.translation_layouts(),
        &inline_text_catalog_sha256,
        inline_text_catalog.translation_layouts(),
    )?;
    ensure!(
        loaded_translation_catalog.entry_count == draft.entry_count(),
        "translation catalog does not own every extracted text entry"
    );

    let mado456_ui = Mado456UiTranslation::load()?;
    let mut required_external_characters = loaded_translation_catalog
        .required_external_characters()
        .clone();
    required_external_characters.extend(mado456_ui.required_external_characters());
    let mut source_codes = message_catalog.translation_font_source_codes()?;
    source_codes.extend(inline_text_catalog.translation_font_source_codes());
    let font = build_translation_font_plan(&required_external_characters, &source_codes)?;

    let message_translations = draft.collection_segments(MESSAGE_COLLECTION)?;
    let rebuilt_messages =
        message_catalog.rebuild_with_translations(&message_translations, font.codebook())?;
    let message_buffers = validate_message_buffer_layout(&rebuilt_messages)?;

    let drbios_source = &require_file(&files, "DRBIOS.COM")?.bytes;
    let mado456_source = &require_file(&files, "MADO456.COM")?.bytes;
    let opening_source = &require_file(&files, "OPENING.COM")?.bytes;
    let ending_source = &require_file(&files, "ENDING.COM")?.bytes;
    let (drbios, drbios_patch) = install_drbios_translation_font(drbios_source, &font)?;
    let (mado456_routed, mut mado456_patch) =
        install_mado456_localization_runtime_support(mado456_source)?;
    let (mado456, mado456_ui_report, mado456_ui_used_external_characters) =
        mado456_ui.apply(&mado456_routed, font.codebook())?;
    mado456_patch.updated_sha256 = sha256_hex(&mado456);
    mado456_patch.fixed_expected_write_count += mado456_ui_report.fixed_expected_write_count;
    let (opening_base, opening_patch) =
        route_inline_renderer_through_translation_glyph_service("OPENING.COM", opening_source)?;
    let (ending_base, ending_patch) =
        route_inline_renderer_through_translation_glyph_service("ENDING.COM", ending_source)?;
    let inline_bases = BTreeMap::from([
        ("OPENING.COM".to_owned(), opening_base),
        ("ENDING.COM".to_owned(), ending_base),
    ]);
    let inline_translations = draft.collection_segments(INLINE_TEXT_COLLECTION)?;
    let rebuilt_inline = inline_text_catalog.rebuild_with_translations(
        &files,
        &inline_bases,
        &inline_translations,
        font.codebook(),
    )?;

    let used_external_characters = rebuilt_messages
        .iter()
        .flat_map(|file| file.used_external_characters.iter().copied())
        .chain(
            rebuilt_inline
                .iter()
                .flat_map(|program| program.used_external_characters.iter().copied()),
        )
        .chain(mado456_ui_used_external_characters)
        .collect::<BTreeSet<_>>();
    ensure!(
        used_external_characters == required_external_characters,
        "localized files do not consume exactly the translation-derived external glyph set"
    );

    let renderer_patches = vec![drbios_patch, mado456_patch, opening_patch, ending_patch];
    let mut output_files = BTreeMap::new();
    let mut file_reports = Vec::new();
    add_message_files(
        &files,
        &rebuilt_messages,
        &mut output_files,
        &mut file_reports,
    )?;
    add_renderer_file(
        "DRBIOS.COM",
        drbios_source,
        drbios,
        &renderer_patches[0],
        &mut output_files,
        &mut file_reports,
    )?;
    add_renderer_file(
        "MADO456.COM",
        mado456_source,
        mado456,
        &renderer_patches[1],
        &mut output_files,
        &mut file_reports,
    )?;
    let mado456_file_report = file_reports
        .iter_mut()
        .find(|report| report.name == "MADO456.COM")
        .context("localized file report lost MADO456.COM")?;
    mado456_file_report.role =
        "translated fixed game UI, expanded message buffers, and glyph renderer";
    mado456_file_report.changed_entry_count = mado456_ui_report.translated_occurrence_count;
    add_inline_files(
        &files,
        &rebuilt_inline,
        &renderer_patches[2..],
        &mut output_files,
        &mut file_reports,
    )?;

    let graphics = build_graphic_localization(&files, &output_files)?;
    merge_graphic_files(
        &files,
        graphics.replacements,
        &graphics.changed_unit_counts,
        &graphics.fixed_expected_write_counts,
        &mut output_files,
        &mut file_reports,
    )?;

    let translation_catalog_bytes = fs::read(translation_catalog_path).with_context(|| {
        format!(
            "could not read translation catalog {}",
            translation_catalog_path.display()
        )
    })?;
    let manifest = LocalizedGameFilesManifest {
        schema: "pc98_madou456.localized_game_files",
        translation_catalog: InputAsset {
            path: translation_catalog_path.to_path_buf(),
            sha256: sha256_hex(&translation_catalog_bytes),
            entry_count: loaded_translation_catalog.entry_count,
        },
        source_catalogs: [
            SourceCatalogAsset {
                id: MESSAGE_COLLECTION,
                sha256: message_catalog_sha256,
                entry_count: message_catalog.message_entry_count,
            },
            SourceCatalogAsset {
                id: INLINE_TEXT_COLLECTION,
                sha256: inline_text_catalog_sha256,
                entry_count: inline_text_catalog.analysis.entry_count,
            },
        ],
        font: font.report().clone(),
        message_buffers,
        mado456_ui: mado456_ui_report.clone(),
        graphic_text: graphics.report.clone(),
        preserved_translation_font_codes: source_codes.iter().map(hex_pair).collect(),
        renderer_patches,
        files: file_reports,
        limitations: [
            "these are private replacement game files and not an HDI build",
            "typed hooks, Expected Writes, serialized readback, and glyph coverage do not prove screen layout",
            "runtime presentation remains unverified for this exact artifact; CFG_S.DAT remains unchanged because no verified consumer is linked",
            "development-build and distribution eligibility are not assigned by this command",
        ],
    };

    let parent = output_directory
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent).with_context(|| {
        format!(
            "could not create localized-game-files parent directory {}",
            parent.display()
        )
    })?;
    let staging = Builder::new()
        .prefix(".madou456-localized-game-files-")
        .tempdir_in(parent)
        .with_context(|| {
            format!(
                "could not create localized-game-files staging directory in {}",
                parent.display()
            )
        })?;
    for (name, bytes) in &output_files {
        let path = staging.path().join(name);
        write_binary_noclobber(&path, bytes)?;
        ensure!(
            fs::read(&path)? == *bytes,
            "localized game file {name} changed after writing"
        );
    }
    let manifest_bytes = serde_json::to_vec_pretty(&manifest)?;
    let manifest_path = staging.path().join(MANIFEST_FILE);
    write_binary_noclobber(&manifest_path, &manifest_bytes)?;
    let manifest_sha256 = sha256_hex(&manifest_bytes);
    let staging_path = staging.keep();
    fs::rename(&staging_path, output_directory).with_context(|| {
        format!(
            "could not publish localized-game-files directory {}",
            output_directory.display()
        )
    })?;

    let changed_entry_count = rebuilt_messages
        .iter()
        .map(|file| file.changed_entry_count)
        .sum::<usize>()
        + rebuilt_inline
            .iter()
            .map(|program| program.changed_entry_count)
            .sum::<usize>()
        + mado456_ui_report.translated_occurrence_count;
    Ok(LocalizedGameFilesWriteReport {
        output_directory: output_directory.to_path_buf(),
        manifest_sha256,
        translation_catalog: translation_catalog_path.to_path_buf(),
        translation_catalog_sha256: sha256_hex(&translation_catalog_bytes),
        translation_entry_count: loaded_translation_catalog.entry_count
            + mado456_ui_report.entry_count,
        external_character_count: used_external_characters.len(),
        preserved_translation_font_code_count: source_codes.len(),
        file_count: output_files.len(),
        changed_entry_count,
        graphic_translation_unit_count: graphics.report.translated_unit_count,
        graphic_preserve_source_unit_count: graphics.report.preserve_source_unit_count,
        status: "all tracked game-text translations were encoded, rebuilt, and read back with a collision-aware 940-cell resident font and typed renderer hooks; layout, presentation, runtime, HDI integration, development-build eligibility, and distribution eligibility remain separate",
    })
}

pub fn build_verified_localized_hdi(
    source_cd_path: &Path,
    system_hdi_path: &Path,
    translation_catalog_path: &Path,
    output_path: &Path,
) -> Result<LocalizedHdiBuildReport> {
    ensure!(
        !output_path.exists(),
        "refusing to overwrite existing output {}",
        output_path.display()
    );
    let staging = Builder::new()
        .prefix("madou456-localized-hdi-")
        .tempdir()
        .context("could not create localized HDI staging directory")?;
    let replacement_directory = staging.path().join("game-files");
    let localized = write_verified_localized_game_files(
        source_cd_path,
        system_hdi_path,
        translation_catalog_path,
        &replacement_directory,
    )?;

    let mut replacements = BTreeMap::new();
    for entry in fs::read_dir(&replacement_directory).with_context(|| {
        format!(
            "could not enumerate staged localized files in {}",
            replacement_directory.display()
        )
    })? {
        let entry = entry?;
        ensure!(
            entry.file_type()?.is_file(),
            "localized staging output contains a non-file entry"
        );
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| anyhow::anyhow!("localized staging output has a non-UTF-8 filename"))?;
        if name == MANIFEST_FILE {
            continue;
        }
        ensure!(
            replacements
                .insert(name.clone(), fs::read(entry.path())?)
                .is_none(),
            "localized staging output repeats {name}"
        );
    }
    ensure!(
        replacements.len() == EXPECTED_LOCALIZED_GAME_FILE_COUNT,
        "localized staging output must contain exactly {EXPECTED_LOCALIZED_GAME_FILE_COUNT} replacement files, got {}",
        replacements.len()
    );

    let reassembly = build_reassembled_hdi_with_replacements(
        source_cd_path,
        system_hdi_path,
        output_path,
        &replacements,
    )?;
    Ok(LocalizedHdiBuildReport {
        reassembly,
        localized_game_files_manifest_sha256: localized.manifest_sha256,
        translation_catalog: localized.translation_catalog,
        translation_catalog_sha256: localized.translation_catalog_sha256,
        translation_entry_count: localized.translation_entry_count,
        external_character_count: localized.external_character_count,
        preserved_translation_font_code_count: localized.preserved_translation_font_code_count,
        replacement_file_count: localized.file_count,
        changed_entry_count: localized.changed_entry_count,
        graphic_translation_unit_count: localized.graphic_translation_unit_count,
        graphic_preserve_source_unit_count: localized.graphic_preserve_source_unit_count,
        status: "all tracked game-text translations were rebuilt, inserted into the exact 176-file game set, and re-extracted byte-for-byte from the standalone FAT16 HDI; layout, presentation, runtime, human approval, and distribution eligibility remain separate",
    })
}

fn merge_graphic_files(
    sources: &[crate::source_cd::GameFile],
    replacements: BTreeMap<String, Vec<u8>>,
    changed_unit_counts: &BTreeMap<String, usize>,
    fixed_expected_write_counts: &BTreeMap<String, usize>,
    output_files: &mut BTreeMap<String, Vec<u8>>,
    reports: &mut Vec<LocalizedGameFileReport>,
) -> Result<()> {
    for (name, bytes) in replacements {
        let source = require_file(sources, &name)?;
        let changed_unit_count = *changed_unit_counts
            .get(&name)
            .with_context(|| format!("graphic output {name} has no unit ownership count"))?;
        let expected_write_count = fixed_expected_write_counts.get(&name).copied().unwrap_or(1);
        output_files.insert(name.clone(), bytes.clone());
        if let Some(report) = reports.iter_mut().find(|report| report.name == name) {
            report.output_size = bytes.len();
            report.output_sha256 = sha256_hex(&bytes);
            report.changed_entry_count += changed_unit_count;
            report.fixed_expected_write_count += expected_write_count;
            report.role = "translated text, renderer, and graphic descriptors";
        } else {
            reports.push(LocalizedGameFileReport {
                name: name.clone(),
                role: if name.ends_with(".DAT") {
                    "translated packed graphic tile atlas"
                } else {
                    "translated graphic tile-map descriptors"
                },
                source_size: source.bytes.len(),
                source_sha256: sha256_hex(&source.bytes),
                output_size: bytes.len(),
                output_sha256: sha256_hex(&bytes),
                changed_entry_count: changed_unit_count,
                fixed_expected_write_count: expected_write_count,
                appended_offset: None,
                appended_size: 0,
            });
        }
    }
    reports.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(())
}

fn add_message_files(
    sources: &[crate::source_cd::GameFile],
    rebuilt: &[RebuiltMessageFile],
    output_files: &mut BTreeMap<String, Vec<u8>>,
    reports: &mut Vec<LocalizedGameFileReport>,
) -> Result<()> {
    for file in rebuilt {
        let source = require_file(sources, &file.name)?;
        ensure!(
            output_files
                .insert(file.name.clone(), file.packed.clone())
                .is_none(),
            "localized output repeats {}",
            file.name
        );
        reports.push(LocalizedGameFileReport {
            name: file.name.clone(),
            role: "translated packed message table",
            source_size: source.bytes.len(),
            source_sha256: sha256_hex(&source.bytes),
            output_size: file.packed.len(),
            output_sha256: sha256_hex(&file.packed),
            changed_entry_count: file.changed_entry_count,
            fixed_expected_write_count: 1,
            appended_offset: None,
            appended_size: 0,
        });
    }
    Ok(())
}

fn add_renderer_file(
    name: &str,
    source: &[u8],
    output: Vec<u8>,
    patch: &RendererPatchReport,
    output_files: &mut BTreeMap<String, Vec<u8>>,
    reports: &mut Vec<LocalizedGameFileReport>,
) -> Result<()> {
    ensure!(
        patch.program == name,
        "renderer patch report changed program identity"
    );
    ensure!(
        output_files
            .insert(name.to_owned(), output.clone())
            .is_none(),
        "localized output repeats {name}"
    );
    reports.push(LocalizedGameFileReport {
        name: name.to_owned(),
        role: "translation glyph renderer",
        source_size: source.len(),
        source_sha256: sha256_hex(source),
        output_size: output.len(),
        output_sha256: sha256_hex(&output),
        changed_entry_count: 0,
        fixed_expected_write_count: patch.fixed_expected_write_count,
        appended_offset: (patch.appended_size > 0).then_some(patch.appended_offset),
        appended_size: patch.appended_size,
    });
    Ok(())
}

fn add_inline_files(
    sources: &[crate::source_cd::GameFile],
    rebuilt: &[RebuiltInlineProgram],
    patches: &[RendererPatchReport],
    output_files: &mut BTreeMap<String, Vec<u8>>,
    reports: &mut Vec<LocalizedGameFileReport>,
) -> Result<()> {
    ensure!(
        rebuilt.len() == patches.len(),
        "inline renderer patch and rebuilt-program populations differ"
    );
    for program in rebuilt {
        let source = require_file(sources, &program.name)?;
        let patch = patches
            .iter()
            .find(|patch| patch.program == program.name)
            .with_context(|| format!("inline renderer patch lacks {}", program.name))?;
        ensure!(
            output_files
                .insert(program.name.clone(), program.bytes.clone())
                .is_none(),
            "localized output repeats {}",
            program.name
        );
        reports.push(LocalizedGameFileReport {
            name: program.name.clone(),
            role: "translated relocated inline text and glyph renderer",
            source_size: source.bytes.len(),
            source_sha256: sha256_hex(&source.bytes),
            output_size: program.bytes.len(),
            output_sha256: sha256_hex(&program.bytes),
            changed_entry_count: program.changed_entry_count,
            fixed_expected_write_count: patch.fixed_expected_write_count
                + program.pointer_expected_write_count,
            appended_offset: Some(patch.appended_offset),
            appended_size: patch.appended_size + program.pool_size,
        });
        ensure!(
            program.pool_file_offset == patch.updated_size,
            "{} inline pool no longer follows its typed renderer hook",
            program.name
        );
    }
    Ok(())
}

fn write_binary_noclobber(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .with_context(|| format!("could not create {}", path.display()))?;
    output
        .write_all(bytes)
        .with_context(|| format!("could not write {}", path.display()))?;
    output
        .sync_all()
        .with_context(|| format!("could not sync {}", path.display()))
}

fn hex_pair(pair: &[u8; 2]) -> String {
    format!("{:02x}{:02x}", pair[0], pair[1])
}
