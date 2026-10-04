use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, ensure};
use serde::Serialize;
use tempfile::Builder;

use crate::compile_lz::decode_exact_compile_lz;
use crate::reassembly::load_verified_game_files;

const EXPECTED_DECODED_FILE_COUNT: usize = 166;
const EXPECTED_DECODED_STREAM_COUNT: usize = 166;
const EXPECTED_DECODED_TOTAL_SIZE: usize = 3_335_018;

#[derive(Debug, Serialize)]
pub struct GameFileExtractionReport {
    output_directory: PathBuf,
    file_count: usize,
    total_size: usize,
}

#[derive(Debug, Serialize)]
pub struct DecodedGameFileExtractionReport {
    output_directory: PathBuf,
    file_count: usize,
    stream_count: usize,
    total_size: usize,
    files: Vec<DecodedGameFileReport>,
}

#[derive(Debug, Serialize)]
struct DecodedGameFileReport {
    name: String,
    packed_size: usize,
    decoded_size: usize,
    decoded_sha256: String,
}

pub fn extract_verified_game_files(
    source_cd_path: &Path,
    system_hdi_path: &Path,
    output_directory: &Path,
) -> Result<GameFileExtractionReport> {
    ensure!(
        !output_directory.exists(),
        "refusing to overwrite existing extraction directory {}",
        output_directory.display()
    );
    let files = load_verified_game_files(source_cd_path, system_hdi_path)?;
    let parent = output_directory
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent).with_context(|| {
        format!(
            "could not create extraction parent directory {}",
            parent.display()
        )
    })?;
    let staging = Builder::new()
        .prefix(".madou456-game-files-")
        .tempdir_in(parent)
        .with_context(|| {
            format!(
                "could not create extraction staging directory in {}",
                parent.display()
            )
        })?;

    let mut total_size = 0usize;
    for game_file in &files {
        let output_path = staging.path().join(&game_file.display_name);
        let mut output = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&output_path)
            .with_context(|| format!("could not create {}", output_path.display()))?;
        output
            .write_all(&game_file.bytes)
            .with_context(|| format!("could not write {}", output_path.display()))?;
        total_size += game_file.bytes.len();
    }

    let staging_path = staging.keep();
    fs::rename(&staging_path, output_directory).with_context(|| {
        format!(
            "could not publish extraction directory {}",
            output_directory.display()
        )
    })?;

    Ok(GameFileExtractionReport {
        output_directory: output_directory.to_path_buf(),
        file_count: files.len(),
        total_size,
    })
}

pub fn extract_verified_decoded_game_files(
    source_cd_path: &Path,
    system_hdi_path: &Path,
    output_directory: &Path,
) -> Result<DecodedGameFileExtractionReport> {
    ensure!(
        !output_directory.exists(),
        "refusing to overwrite existing decoded extraction directory {}",
        output_directory.display()
    );
    let files = load_verified_game_files(source_cd_path, system_hdi_path)?;
    let parent = output_directory
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent).with_context(|| {
        format!(
            "could not create decoded extraction parent directory {}",
            parent.display()
        )
    })?;
    let staging = Builder::new()
        .prefix(".madou456-decoded-files-")
        .tempdir_in(parent)
        .with_context(|| {
            format!(
                "could not create decoded extraction staging directory in {}",
                parent.display()
            )
        })?;

    let mut file_reports = Vec::new();
    let mut stream_count = 0usize;
    let mut total_size = 0usize;
    for game_file in &files {
        let Some(mut decoded) = decode_exact_compile_lz(&game_file.bytes) else {
            continue;
        };
        ensure!(
            decoded.streams.len() == 1,
            "{} unexpectedly contains {} exact Compile-LZ streams",
            game_file.display_name,
            decoded.streams.len()
        );
        let bytes = decoded.streams.remove(0);
        let output_path = staging.path().join(&game_file.display_name);
        let mut output = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&output_path)
            .with_context(|| format!("could not create {}", output_path.display()))?;
        output
            .write_all(&bytes)
            .with_context(|| format!("could not write {}", output_path.display()))?;
        output
            .sync_all()
            .with_context(|| format!("could not sync {}", output_path.display()))?;
        let written = fs::read(&output_path)
            .with_context(|| format!("could not verify {}", output_path.display()))?;
        ensure!(
            written == bytes,
            "decoded extraction differs after writing {}",
            output_path.display()
        );

        stream_count += decoded.report.streams.len();
        total_size += bytes.len();
        file_reports.push(DecodedGameFileReport {
            name: game_file.display_name.clone(),
            packed_size: game_file.bytes.len(),
            decoded_size: bytes.len(),
            decoded_sha256: decoded.report.streams[0].decoded_sha256.clone(),
        });
    }

    ensure!(
        file_reports.len() == EXPECTED_DECODED_FILE_COUNT,
        "decoded file population changed: expected {EXPECTED_DECODED_FILE_COUNT}, got {}",
        file_reports.len()
    );
    ensure!(
        stream_count == EXPECTED_DECODED_STREAM_COUNT,
        "decoded stream population changed: expected {EXPECTED_DECODED_STREAM_COUNT}, got {stream_count}"
    );
    ensure!(
        total_size == EXPECTED_DECODED_TOTAL_SIZE,
        "decoded size population changed: expected {EXPECTED_DECODED_TOTAL_SIZE}, got {total_size}"
    );

    let staging_path = staging.keep();
    fs::rename(&staging_path, output_directory).with_context(|| {
        format!(
            "could not publish decoded extraction directory {}",
            output_directory.display()
        )
    })?;

    Ok(DecodedGameFileExtractionReport {
        output_directory: output_directory.to_path_buf(),
        file_count: file_reports.len(),
        stream_count,
        total_size,
        files: file_reports,
    })
}
