use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, ensure};
use encoding_rs::SHIFT_JIS;
use serde::{Deserialize, Serialize};
use tempfile::Builder;

use crate::reassembly::load_verified_game_files;

use super::translation_draft::{TranslationDraft, audit_translation_draft, serialized_sha256};
use super::translation_overlay::{
    LoadedTranslationOverlayCatalog, load_translation_overlay_catalog,
};
use super::{
    MADO456_SHA256, catalog_inline_text, catalog_messages, sha256_hex, write_pretty_json_noclobber,
};

const CODEBOOK_SCHEMA: &str = "pc98_madou456.translation_codebook";
const MESSAGE_COLLECTION: &str = "messages";
const MANIFEST_FILE: &str = "manifest.json";

#[derive(Debug, Serialize)]
pub struct MessageReinsertionWriteReport {
    output_directory: PathBuf,
    manifest_sha256: String,
    draft: PathBuf,
    draft_sha256: String,
    translation_catalog: Option<PathBuf>,
    translation_catalog_sha256: Option<String>,
    translation_entry_count: usize,
    codebook: Option<PathBuf>,
    codebook_sha256: Option<String>,
    codebook_entry_count: usize,
    used_external_character_count: usize,
    file_count: usize,
    changed_file_count: usize,
    changed_entry_count: usize,
    all_source_packed_reproduced: bool,
    status: &'static str,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TranslationCodebook {
    schema: String,
    source_program: SourceProgramBinding,
    code_space: String,
    entries: Vec<CodebookEntry>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SourceProgramBinding {
    name: String,
    sha256: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CodebookEntry {
    character: String,
    encoded_hex: String,
}

#[derive(Debug, Serialize)]
struct MessageReinsertionManifest {
    schema: &'static str,
    draft: InputAsset,
    translation_catalog: Option<InputAsset>,
    codebook: Option<InputAsset>,
    files: Vec<RebuiltMessageFileReport>,
    used_external_characters: Vec<String>,
    limitations: [&'static str; 4],
}

#[derive(Clone, Debug, Serialize)]
struct InputAsset {
    path: PathBuf,
    sha256: String,
    entry_count: usize,
}

#[derive(Debug, Serialize)]
struct RebuiltMessageFileReport {
    name: String,
    decoded_size: usize,
    decoded_sha256: String,
    packed_size: usize,
    packed_sha256: String,
    changed_entry_count: usize,
    source_packed_reproduced: bool,
    exact_compile_lz_roundtrip: bool,
}

pub fn write_verified_rebuilt_message_files(
    source_cd_path: &Path,
    system_hdi_path: &Path,
    draft_path: &Path,
    translation_catalog_path: Option<&Path>,
    codebook_path: Option<&Path>,
    output_directory: &Path,
) -> Result<MessageReinsertionWriteReport> {
    ensure!(
        !output_directory.exists(),
        "refusing to overwrite existing rebuilt-message directory {}",
        output_directory.display()
    );
    let files = load_verified_game_files(source_cd_path, system_hdi_path)?;
    let message_catalog = catalog_messages(&files)?;
    let inline_text_catalog = catalog_inline_text(&files)?;
    let message_catalog_sha256 = serialized_sha256(&message_catalog)?;
    let inline_text_catalog_sha256 = serialized_sha256(&inline_text_catalog)?;

    let draft_bytes = fs::read(draft_path)
        .with_context(|| format!("could not read translation draft {}", draft_path.display()))?;
    let mut draft: TranslationDraft = serde_json::from_slice(&draft_bytes)
        .with_context(|| format!("could not parse translation draft {}", draft_path.display()))?;
    audit_translation_draft(
        &draft,
        &message_catalog_sha256,
        message_catalog.translation_layouts(),
        &inline_text_catalog_sha256,
        inline_text_catalog.translation_layouts(),
    )?;
    let loaded_translation_catalog = match translation_catalog_path {
        Some(translation_catalog_path) => {
            let loaded = load_translation_overlay_catalog(
                translation_catalog_path,
                &message_catalog_sha256,
                &message_catalog,
                &inline_text_catalog_sha256,
                &inline_text_catalog,
            )?;
            loaded.apply_to_draft(&mut draft)?;
            Some(loaded)
        }
        None => None,
    };
    audit_translation_draft(
        &draft,
        &message_catalog_sha256,
        message_catalog.translation_layouts(),
        &inline_text_catalog_sha256,
        inline_text_catalog.translation_layouts(),
    )?;
    let translations = draft.collection_segments(MESSAGE_COLLECTION)?;

    let loaded_codebook = load_codebook(codebook_path)?;
    let rebuilt = message_catalog
        .rebuild_with_translations(&translations, &loaded_codebook.character_codes)?;
    let used_external_characters = rebuilt
        .iter()
        .flat_map(|file| file.used_external_characters.iter().copied())
        .collect::<BTreeSet<_>>();

    let parent = output_directory
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent).with_context(|| {
        format!(
            "could not create rebuilt-message parent directory {}",
            parent.display()
        )
    })?;
    let staging = Builder::new()
        .prefix(".madou456-rebuilt-messages-")
        .tempdir_in(parent)
        .with_context(|| {
            format!(
                "could not create rebuilt-message staging directory in {}",
                parent.display()
            )
        })?;

    let mut file_reports = Vec::with_capacity(rebuilt.len());
    for file in &rebuilt {
        let output_path = staging.path().join(&file.name);
        write_binary_noclobber(&output_path, &file.packed)?;
        let written = fs::read(&output_path)
            .with_context(|| format!("could not verify rebuilt {}", output_path.display()))?;
        ensure!(
            written == file.packed,
            "rebuilt message file {} changed after writing",
            file.name
        );
        file_reports.push(RebuiltMessageFileReport {
            name: file.name.clone(),
            decoded_size: file.decoded.len(),
            decoded_sha256: sha256_hex(&file.decoded),
            packed_size: file.packed.len(),
            packed_sha256: sha256_hex(&file.packed),
            changed_entry_count: file.changed_entry_count,
            source_packed_reproduced: file.source_packed_reproduced,
            exact_compile_lz_roundtrip: true,
        });
    }

    let draft_sha256 = sha256_hex(&draft_bytes);
    let manifest = MessageReinsertionManifest {
        schema: "pc98_madou456.rebuilt_message_files",
        draft: InputAsset {
            path: draft_path.to_path_buf(),
            sha256: draft_sha256.clone(),
            entry_count: draft.entry_count(),
        },
        translation_catalog: loaded_translation_catalog
            .as_ref()
            .map(translation_catalog_input_asset),
        codebook: loaded_codebook.input_asset.clone(),
        files: file_reports,
        used_external_characters: used_external_characters
            .iter()
            .map(char::to_string)
            .collect(),
        limitations: [
            "this output contains only the fifteen packed message files and is not an HDI build",
            "codebook acceptance proves byte serialization but does not prove that a matching glyph is installed",
            "exact Compile-LZ roundtrip does not prove layout or runtime display",
            "development-build and distribution eligibility are not assigned by this command",
        ],
    };
    let manifest_sha256 = write_pretty_json_noclobber(
        &manifest,
        &staging.path().join(MANIFEST_FILE),
        "rebuilt-message manifest",
    )?;

    let staging_path = staging.keep();
    fs::rename(&staging_path, output_directory).with_context(|| {
        format!(
            "could not publish rebuilt-message directory {}",
            output_directory.display()
        )
    })?;

    let changed_file_count = manifest
        .files
        .iter()
        .filter(|file| file.changed_entry_count > 0)
        .count();
    let changed_entry_count = manifest
        .files
        .iter()
        .map(|file| file.changed_entry_count)
        .sum();
    let all_source_packed_reproduced = manifest
        .files
        .iter()
        .all(|file| file.source_packed_reproduced);
    Ok(MessageReinsertionWriteReport {
        output_directory: output_directory.to_path_buf(),
        manifest_sha256,
        draft: draft_path.to_path_buf(),
        draft_sha256,
        translation_catalog: loaded_translation_catalog
            .as_ref()
            .map(|loaded| loaded.catalog.clone()),
        translation_catalog_sha256: loaded_translation_catalog
            .as_ref()
            .map(|loaded| loaded.catalog_sha256.clone()),
        translation_entry_count: loaded_translation_catalog
            .as_ref()
            .map_or(0, |loaded| loaded.entry_count),
        codebook: codebook_path.map(Path::to_path_buf),
        codebook_sha256: loaded_codebook
            .input_asset
            .as_ref()
            .map(|asset| asset.sha256.clone()),
        codebook_entry_count: loaded_codebook.character_codes.len(),
        used_external_character_count: used_external_characters.len(),
        file_count: manifest.files.len(),
        changed_file_count,
        changed_entry_count,
        all_source_packed_reproduced,
        status: "translation segments were reassembled around source-owned controls, all 16-bit offset tables were regenerated, and all fifteen packed files restore their planned decoded bytes; glyph installation, layout, presentation, runtime, HDI integration, development-build eligibility, and distribution eligibility are not proven",
    })
}

fn translation_catalog_input_asset(loaded: &LoadedTranslationOverlayCatalog) -> InputAsset {
    InputAsset {
        path: loaded.catalog.clone(),
        sha256: loaded.catalog_sha256.clone(),
        entry_count: loaded.entry_count,
    }
}

struct LoadedCodebook {
    character_codes: BTreeMap<char, [u8; 2]>,
    input_asset: Option<InputAsset>,
}

fn load_codebook(path: Option<&Path>) -> Result<LoadedCodebook> {
    let Some(path) = path else {
        return Ok(LoadedCodebook {
            character_codes: BTreeMap::new(),
            input_asset: None,
        });
    };
    let bytes = fs::read(path)
        .with_context(|| format!("could not read translation codebook {}", path.display()))?;
    let codebook: TranslationCodebook = serde_json::from_slice(&bytes)
        .with_context(|| format!("could not parse translation codebook {}", path.display()))?;
    let character_codes = validate_codebook(&codebook)?;
    Ok(LoadedCodebook {
        character_codes,
        input_asset: Some(InputAsset {
            path: path.to_path_buf(),
            sha256: sha256_hex(&bytes),
            entry_count: codebook.entries.len(),
        }),
    })
}

fn validate_codebook(codebook: &TranslationCodebook) -> Result<BTreeMap<char, [u8; 2]>> {
    ensure!(
        codebook.schema == CODEBOOK_SCHEMA,
        "unsupported translation codebook schema"
    );
    ensure!(
        codebook.source_program.name == "MADO456.COM"
            && codebook.source_program.sha256 == MADO456_SHA256,
        "translation codebook is not bound to the supported message renderer"
    );
    ensure!(
        codebook.code_space == "static_bios_candidate",
        "translation codebook uses an unsupported code space"
    );
    ensure!(
        !codebook.entries.is_empty(),
        "translation codebook is empty"
    );

    let mut characters = BTreeMap::new();
    let mut codes = BTreeSet::new();
    for entry in &codebook.entries {
        let mut entry_characters = entry.character.chars();
        let character = entry_characters
            .next()
            .context("translation codebook entry has no character")?;
        ensure!(
            entry_characters.next().is_none() && !character.is_control(),
            "translation codebook entries must name one non-control Unicode scalar"
        );
        let source_text = character.to_string();
        let (_, _, is_unavailable_in_cp932) = SHIFT_JIS.encode(&source_text);
        ensure!(
            is_unavailable_in_cp932,
            "translation codebook must not override a character already available in CP932"
        );
        let code = decode_pair(&entry.encoded_hex)?;
        ensure!(
            is_static_bios_candidate(code),
            "translation codebook entry for {character:?} is outside the static BIOS candidate window"
        );
        ensure!(
            characters.insert(character, code).is_none() && codes.insert(code),
            "translation codebook duplicates a character or encoded pair"
        );
    }
    Ok(characters)
}

fn decode_pair(encoded: &str) -> Result<[u8; 2]> {
    ensure!(
        encoded.len() == 4 && encoded.bytes().all(|byte| byte.is_ascii_hexdigit()),
        "translation codebook encoded_hex must contain exactly two bytes"
    );
    let high = u8::from_str_radix(&encoded[..2], 16).context("invalid codebook lead byte")?;
    let low = u8::from_str_radix(&encoded[2..], 16).context("invalid codebook trail byte")?;
    Ok([high, low])
}

fn is_static_bios_candidate([lead, trail]: [u8; 2]) -> bool {
    (lead == 0xeb && (0x9f..=0xfc).contains(&trail))
        || (lead == 0xec && ((0x40..=0x7e).contains(&trail) || (0x80..=0x9e).contains(&trail)))
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

#[cfg(test)]
#[path = "message_reinsertion_tests.rs"]
mod tests;
