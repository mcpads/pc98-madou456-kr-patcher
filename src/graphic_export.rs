use std::collections::BTreeSet;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, ensure};
use serde::Serialize;
use sha2::{Digest, Sha256};
use tempfile::Builder;

use crate::compile_lz::decode_exact_compile_lz;
use crate::masked_tile::{ATLAS_DECODED_SIZE, render_diagnostic_color_ppm, render_mask_ppm};
use crate::reassembly::load_verified_game_files;
use crate::source_cd::GameFile;

#[derive(Debug, Serialize)]
pub struct GraphicAtlasExportReport {
    output_directory: PathBuf,
    atlas_count: usize,
    files: Vec<GraphicAtlasFileReport>,
}

#[derive(Debug, Serialize)]
pub(crate) struct GraphicAtlasFileReport {
    name: String,
    decoded_sha256: String,
    color_ppm: RenderedGraphicReport,
    mask_ppm: RenderedGraphicReport,
}

#[derive(Debug, Serialize)]
struct RenderedGraphicReport {
    file_name: String,
    size: usize,
    sha256: String,
}

pub fn export_verified_masked_tile_atlases(
    source_cd_path: &Path,
    system_hdi_path: &Path,
    requested_names: &[String],
    output_directory: &Path,
) -> Result<GraphicAtlasExportReport> {
    let files = load_verified_game_files(source_cd_path, system_hdi_path)?;
    export_masked_tile_atlases_from_files(&files, requested_names, output_directory)
}

pub(crate) fn export_masked_tile_atlases_from_files(
    files: &[GameFile],
    requested_names: &[String],
    output_directory: &Path,
) -> Result<GraphicAtlasExportReport> {
    ensure!(
        !output_directory.exists(),
        "refusing to overwrite existing graphic export directory {}",
        output_directory.display()
    );
    let requested_names = requested_name_set(requested_names)?;
    let parent = output_directory
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent).with_context(|| {
        format!(
            "could not create graphic export parent directory {}",
            parent.display()
        )
    })?;
    let staging = Builder::new()
        .prefix(".madou456-graphic-export-")
        .tempdir_in(parent)
        .with_context(|| {
            format!(
                "could not create graphic export staging directory in {}",
                parent.display()
            )
        })?;

    let mut matched_requested_names = BTreeSet::<&str>::new();
    let mut reports = Vec::new();
    for file in files {
        if !requested_names.is_empty() && !requested_names.contains(file.display_name.as_str()) {
            continue;
        }
        let Some(mut decoded) = decode_exact_compile_lz(&file.bytes) else {
            ensure!(
                !requested_names.contains(file.display_name.as_str()),
                "requested file {} is not exact Compile-LZ",
                file.display_name
            );
            continue;
        };
        if decoded.streams.len() != 1 || decoded.streams[0].len() != ATLAS_DECODED_SIZE {
            ensure!(
                !requested_names.contains(file.display_name.as_str()),
                "requested file {} does not decode to one {ATLAS_DECODED_SIZE}-byte atlas",
                file.display_name
            );
            continue;
        }
        matched_requested_names.insert(file.display_name.as_str());
        let atlas = decoded.streams.remove(0);
        let color_ppm = render_diagnostic_color_ppm(&atlas)?;
        let mask_ppm = render_mask_ppm(&atlas)?;
        let color_file_name = format!("{}.color.ppm", file.display_name);
        let mask_file_name = format!("{}.mask.ppm", file.display_name);
        let color_report = write_rendered_graphic(staging.path(), &color_file_name, &color_ppm)?;
        let mask_report = write_rendered_graphic(staging.path(), &mask_file_name, &mask_ppm)?;
        reports.push(GraphicAtlasFileReport {
            name: file.display_name.clone(),
            decoded_sha256: decoded.report.streams[0].decoded_sha256.clone(),
            color_ppm: color_report,
            mask_ppm: mask_report,
        });
    }
    reports.sort_by(|left, right| left.name.cmp(&right.name));
    ensure!(!reports.is_empty(), "no masked tile atlases were selected");
    ensure!(
        requested_names.is_empty() || matched_requested_names == requested_names,
        "one or more requested masked tile atlases were not found"
    );

    let staging_path = staging.keep();
    fs::rename(&staging_path, output_directory).with_context(|| {
        format!(
            "could not publish graphic export directory {}",
            output_directory.display()
        )
    })?;

    Ok(GraphicAtlasExportReport {
        output_directory: output_directory.to_path_buf(),
        atlas_count: reports.len(),
        files: reports,
    })
}

impl GraphicAtlasExportReport {
    pub(crate) fn atlas_count(&self) -> usize {
        self.atlas_count
    }

    pub(crate) fn into_files(self) -> Vec<GraphicAtlasFileReport> {
        self.files
    }
}

fn requested_name_set(requested_names: &[String]) -> Result<BTreeSet<&str>> {
    let names: BTreeSet<&str> = requested_names.iter().map(String::as_str).collect();
    ensure!(
        names.len() == requested_names.len(),
        "duplicate graphic atlas name requested"
    );
    for name in &names {
        ensure!(
            !name.is_empty()
                && !name.contains('/')
                && !name.contains('\\')
                && name.bytes().all(|byte| byte.is_ascii()),
            "invalid graphic atlas name {name:?}"
        );
    }
    Ok(names)
}

fn write_rendered_graphic(
    directory: &Path,
    file_name: &str,
    bytes: &[u8],
) -> Result<RenderedGraphicReport> {
    let path = directory.join(file_name);
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .with_context(|| format!("could not create {}", path.display()))?;
    output
        .write_all(bytes)
        .with_context(|| format!("could not write {}", path.display()))?;
    output
        .sync_all()
        .with_context(|| format!("could not sync {}", path.display()))?;
    let written =
        fs::read(&path).with_context(|| format!("could not verify {}", path.display()))?;
    ensure!(
        written == bytes,
        "rendered graphic differs after writing {}",
        path.display()
    );
    Ok(RenderedGraphicReport {
        file_name: file_name.to_owned(),
        size: bytes.len(),
        sha256: sha256_hex(bytes),
    })
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[cfg(test)]
#[path = "graphic_export_tests.rs"]
mod tests;
