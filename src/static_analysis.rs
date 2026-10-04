use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, ensure};
use serde::Serialize;
use tempfile::NamedTempFile;

use crate::compile_lz::{ExactCompileLzReport, decode_exact_compile_lz, encode_compile_lz};
use crate::localization::{
    COM_LOAD_ORIGIN, DRBIOS_SHA256, ENDING_SHA256, FIRST_GLYPH_TABLE_FILE_OFFSET,
    InlineTextAnalysisReport, MADO456_SHA256, MessageControlLayoutReport, MessageFileSummary,
    MessageFormatReport, OPENING_SHA256, SECOND_GLYPH_TABLE_FILE_OFFSET, catalog_inline_text,
    catalog_messages, hex_bytes, message_format_report, read_u16_le, require_file, require_sha256,
    sha256_hex, translation_draft, translation_overlay,
};
use crate::reassembly::{SourceReport, load_verified_game_files, verify_sources};

#[path = "graphic_analysis.rs"]
mod graphic_analysis;
#[path = "graphic_text_review.rs"]
mod graphic_text_review;
#[path = "graphic_text_usage.rs"]
mod graphic_text_usage;
#[path = "indexed_map_graphic_usage.rs"]
mod indexed_map_graphic_usage;
#[path = "launch_chain_analysis.rs"]
mod launch_chain_analysis;
#[path = "message_reinsertion.rs"]
mod message_reinsertion;
#[path = "text_renderer_analysis.rs"]
mod text_renderer_analysis;
#[path = "translation_workspace.rs"]
mod translation_workspace;
use graphic_analysis::{
    GraphicMemoryLayout, MaskedTileGraphicAnalysis, PlanarGraphicSet, ProgramFilenameTable,
    analyze_graphics,
};
use launch_chain_analysis::{LaunchChainAnalysis, analyze_launch_chain};
pub use message_reinsertion::{
    MessageReinsertionWriteReport, write_verified_rebuilt_message_files,
};
use text_renderer_analysis::{TextRendererAnalysis, analyze_text_renderers};
pub use translation_workspace::{
    TranslationWorkspaceWriteReport, write_verified_translation_workspace,
};

const FPLAY_SHA256: &str = "8004b53936793170af03b628837f972d949fd43da6e2bfa0626a7b403382de08";
const EXPECTED_COMPILE_LZ_FILE_COUNT: usize = 166;
const EXPECTED_COMPILE_LZ_STREAM_COUNT: usize = 166;
const EXPECTED_COMPILE_LZ_DECODED_SIZE: usize = 3_335_018;
const EXPECTED_COMPILE_LZ_SOURCE_PACKED_REPRODUCTION_COUNT: usize = 156;

const EXPECTED_NON_COMPILE_LZ_FILES: [&str; 10] = [
    "456.BAT",
    "DRBIOS.COM",
    "DRSHELL.COM",
    "ENDING.COM",
    "FPLAY.COM",
    "MADO456.COM",
    "MADOU456.BAT",
    "MDSC.COM",
    "OPENING.COM",
    "SONG.DAT",
];

#[derive(Debug, Serialize)]
pub struct GameStaticAnalysisReport {
    game_file_count: usize,
    game_total_size: usize,
    compile_lz_file_count: usize,
    compile_lz_stream_count: usize,
    compile_lz_decoded_size: usize,
    compile_lz_repacked_self_roundtrip_count: usize,
    compile_lz_source_packed_reproduction_count: usize,
    non_compile_lz_files: Vec<String>,
    files: Vec<GameFileStaticReport>,
    message_format: MessageFormatReport,
    message_file_count: usize,
    message_entry_count: usize,
    message_empty_segment_reassembly_identical: bool,
    message_files: Vec<MessageFileSummary>,
    message_control_layout: MessageControlLayoutReport,
    inline_text: InlineTextAnalysisReport,
    text_renderers: TextRendererAnalysis,
    launch_chain: LaunchChainAnalysis,
    program_filename_tables: Vec<ProgramFilenameTable>,
    graphic_memory_layouts: Vec<GraphicMemoryLayout>,
    planar_graphic_sets: Vec<PlanarGraphicSet>,
    masked_tile_graphics: MaskedTileGraphicAnalysis,
    consumer_evidence: Vec<ConsumerEvidence>,
    exact_binary_reuse: Vec<ExactBinaryReuse>,
    unresolved_filename_literals: Vec<FilenameLiteral>,
}

#[derive(Debug, Serialize)]
struct GameFileStaticReport {
    name: String,
    packed_size: usize,
    packed_sha256: String,
    compile_lz: Option<ExactCompileLzReport>,
    compile_lz_repack: Option<CompileLzRepackReport>,
}

#[derive(Debug, Serialize)]
struct CompileLzRepackReport {
    repacked_size: usize,
    repacked_sha256: String,
    self_roundtrip: bool,
    source_packed_reproduced: bool,
}

#[derive(Debug, Serialize)]
struct ConsumerEvidence {
    program: &'static str,
    file_offset: usize,
    runtime_address: usize,
    observation: &'static str,
}

#[derive(Debug, Serialize)]
struct ExactBinaryReuse {
    name: &'static str,
    sha256: &'static str,
    matches: &'static str,
    implication: &'static str,
}

#[derive(Debug, Serialize)]
struct FilenameLiteral {
    name: &'static str,
    file_offset: usize,
    runtime_address: usize,
    status: &'static str,
}

#[derive(Debug, Serialize)]
pub struct MessageCatalogWriteReport {
    output: PathBuf,
    catalog_sha256: String,
    message_file_count: usize,
    message_entry_count: usize,
}

#[derive(Debug, Serialize)]
struct VerifiedGameAssetCatalog {
    sources: SourceReport,
    analysis: GameStaticAnalysisReport,
}

#[derive(Debug, Serialize)]
pub struct GameAssetCatalogWriteReport {
    output: PathBuf,
    catalog_sha256: String,
    game_file_count: usize,
    compile_lz_file_count: usize,
    message_entry_count: usize,
}

#[derive(Debug, Serialize)]
pub struct InlineTextCatalogWriteReport {
    output: PathBuf,
    catalog_sha256: String,
    block_count: usize,
    entry_count: usize,
}

pub fn analyze_verified_game(
    source_cd_path: &Path,
    system_hdi_path: &Path,
) -> Result<GameStaticAnalysisReport> {
    let files = load_verified_game_files(source_cd_path, system_hdi_path)?;
    let main_program = require_file(&files, "MADO456.COM")?;
    require_sha256("MADO456.COM", &main_program.bytes, MADO456_SHA256)?;
    let playback_driver = require_file(&files, "FPLAY.COM")?;
    require_sha256("FPLAY.COM", &playback_driver.bytes, FPLAY_SHA256)?;

    let mut file_reports = Vec::with_capacity(files.len());
    let mut compile_lz_file_count = 0usize;
    let mut compile_lz_stream_count = 0usize;
    let mut compile_lz_decoded_size = 0usize;
    let mut compile_lz_repacked_self_roundtrip_count = 0usize;
    let mut compile_lz_source_packed_reproduction_count = 0usize;
    let mut non_compile_lz_files = Vec::new();
    for file in &files {
        let decoded = decode_exact_compile_lz(&file.bytes);
        let (compile_lz, compile_lz_repack) = if let Some(decoded) = decoded {
            let mut repacked = Vec::new();
            for stream in &decoded.streams {
                repacked.extend_from_slice(&encode_compile_lz(stream));
            }
            let repacked_decoded = decode_exact_compile_lz(&repacked).with_context(|| {
                format!("repacked {} is not exact Compile-LZ", file.display_name)
            })?;
            let self_roundtrip = repacked_decoded.streams == decoded.streams;
            ensure!(
                self_roundtrip,
                "repacked {} does not restore its decoded streams",
                file.display_name
            );
            let source_packed_reproduced = repacked == file.bytes;
            compile_lz_repacked_self_roundtrip_count += usize::from(self_roundtrip);
            compile_lz_source_packed_reproduction_count += usize::from(source_packed_reproduced);
            let repack_report = CompileLzRepackReport {
                repacked_size: repacked.len(),
                repacked_sha256: sha256_hex(&repacked),
                self_roundtrip,
                source_packed_reproduced,
            };
            (Some(decoded.report), Some(repack_report))
        } else {
            (None, None)
        };
        if let Some(report) = &compile_lz {
            compile_lz_file_count += 1;
            compile_lz_stream_count += report.streams.len();
            compile_lz_decoded_size += report.decoded_size;
        } else {
            non_compile_lz_files.push(file.display_name.clone());
        }
        file_reports.push(GameFileStaticReport {
            name: file.display_name.clone(),
            packed_size: file.bytes.len(),
            packed_sha256: sha256_hex(&file.bytes),
            compile_lz,
            compile_lz_repack,
        });
    }
    non_compile_lz_files.sort();
    ensure!(
        compile_lz_file_count == EXPECTED_COMPILE_LZ_FILE_COUNT,
        "target Compile-LZ file population changed: expected {EXPECTED_COMPILE_LZ_FILE_COUNT}, got {compile_lz_file_count}"
    );
    ensure!(
        compile_lz_stream_count == EXPECTED_COMPILE_LZ_STREAM_COUNT,
        "target Compile-LZ stream population changed: expected {EXPECTED_COMPILE_LZ_STREAM_COUNT}, got {compile_lz_stream_count}"
    );
    ensure!(
        compile_lz_decoded_size == EXPECTED_COMPILE_LZ_DECODED_SIZE,
        "target Compile-LZ decoded population changed: expected {EXPECTED_COMPILE_LZ_DECODED_SIZE}, got {compile_lz_decoded_size}"
    );
    ensure!(
        compile_lz_repacked_self_roundtrip_count == EXPECTED_COMPILE_LZ_FILE_COUNT,
        "target Compile-LZ repack round-trip population changed: expected {EXPECTED_COMPILE_LZ_FILE_COUNT}, got {compile_lz_repacked_self_roundtrip_count}"
    );
    ensure!(
        compile_lz_source_packed_reproduction_count
            == EXPECTED_COMPILE_LZ_SOURCE_PACKED_REPRODUCTION_COUNT,
        "target Compile-LZ source-packed reproduction population changed: expected {EXPECTED_COMPILE_LZ_SOURCE_PACKED_REPRODUCTION_COUNT}, got {compile_lz_source_packed_reproduction_count}"
    );
    ensure!(
        non_compile_lz_files
            == EXPECTED_NON_COMPILE_LZ_FILES
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>(),
        "target non-Compile-LZ file population changed"
    );
    let compile_lz_reports: BTreeMap<&str, &ExactCompileLzReport> = file_reports
        .iter()
        .filter_map(|file| {
            file.compile_lz
                .as_ref()
                .map(|report| (file.name.as_str(), report))
        })
        .collect();

    let catalog = catalog_messages(&files)?;
    let message_files = catalog
        .files
        .iter()
        .map(|file| MessageFileSummary {
            name: file.name.clone(),
            packed_size: file.packed_size,
            decoded_size: file.decoded_size,
            populated_entries: file.populated_entries,
            decoded_serialization_identical: file.decoded_serialization_identical,
            repacked_size: file.repacked_size,
            repacked_sha256: file.repacked_sha256.clone(),
            repacked_self_roundtrip: file.repacked_self_roundtrip,
            source_packed_reproduced: file.source_packed_reproduced,
        })
        .collect();
    let inline_text_catalog = catalog_inline_text(&files)?;
    let text_renderers = analyze_text_renderers(&files)?;
    let launch_chain = analyze_launch_chain(&files)?;
    let graphics = analyze_graphics(&files, &compile_lz_reports)?;

    let msg14_literal = find_unique_subslice(&main_program.bytes, b"MSG14   .DAT\0")
        .context("MADO456.COM has no unique MSG14.DAT filename literal")?;
    Ok(GameStaticAnalysisReport {
        game_file_count: files.len(),
        game_total_size: files.iter().map(|file| file.bytes.len()).sum(),
        compile_lz_file_count,
        compile_lz_stream_count,
        compile_lz_decoded_size,
        compile_lz_repacked_self_roundtrip_count,
        compile_lz_source_packed_reproduction_count,
        non_compile_lz_files,
        files: file_reports,
        message_format: message_format_report(),
        message_file_count: catalog.message_file_count,
        message_entry_count: catalog.message_entry_count,
        message_empty_segment_reassembly_identical: catalog.empty_segment_reassembly_identical,
        message_files,
        message_control_layout: catalog.control_layout.clone(),
        inline_text: inline_text_catalog.analysis,
        text_renderers,
        launch_chain,
        program_filename_tables: graphics.program_filename_tables,
        graphic_memory_layouts: graphics.graphic_memory_layouts,
        planar_graphic_sets: graphics.planar_graphic_sets,
        masked_tile_graphics: graphics.masked_tile_graphics,
        consumer_evidence: consumer_evidence(),
        exact_binary_reuse: vec![ExactBinaryReuse {
            name: "FPLAY.COM",
            sha256: FPLAY_SHA256,
            matches: "pc98_kikimora verified FPLAY.COM",
            implication: "its existing static text/reference catalog is applicable by exact byte identity",
        }],
        unresolved_filename_literals: vec![FilenameLiteral {
            name: "MSG14.DAT",
            file_offset: msg14_literal,
            runtime_address: msg14_literal + COM_LOAD_ORIGIN,
            status: "referenced by the executable filename table but absent from the verified CD file set; reachability is unresolved",
        }],
    })
}

pub fn write_verified_inline_text_catalog(
    source_cd_path: &Path,
    system_hdi_path: &Path,
    output_path: &Path,
) -> Result<InlineTextCatalogWriteReport> {
    ensure!(
        !output_path.exists(),
        "refusing to overwrite existing inline text catalog {}",
        output_path.display()
    );
    let files = load_verified_game_files(source_cd_path, system_hdi_path)?;
    let catalog = catalog_inline_text(&files)?;
    let catalog_sha256 = write_pretty_json_noclobber(&catalog, output_path, "inline text catalog")?;

    Ok(InlineTextCatalogWriteReport {
        output: output_path.to_path_buf(),
        catalog_sha256,
        block_count: catalog.analysis.block_count,
        entry_count: catalog.analysis.entry_count,
    })
}

pub fn write_verified_message_catalog(
    source_cd_path: &Path,
    system_hdi_path: &Path,
    output_path: &Path,
) -> Result<MessageCatalogWriteReport> {
    ensure!(
        !output_path.exists(),
        "refusing to overwrite existing message catalog {}",
        output_path.display()
    );
    let files = load_verified_game_files(source_cd_path, system_hdi_path)?;
    let catalog = catalog_messages(&files)?;
    let catalog_sha256 = write_pretty_json_noclobber(&catalog, output_path, "message catalog")?;

    Ok(MessageCatalogWriteReport {
        output: output_path.to_path_buf(),
        catalog_sha256,
        message_file_count: catalog.message_file_count,
        message_entry_count: catalog.message_entry_count,
    })
}

pub fn write_verified_game_asset_catalog(
    source_cd_path: &Path,
    system_hdi_path: &Path,
    output_path: &Path,
) -> Result<GameAssetCatalogWriteReport> {
    let catalog = VerifiedGameAssetCatalog {
        sources: verify_sources(source_cd_path, system_hdi_path)?,
        analysis: analyze_verified_game(source_cd_path, system_hdi_path)?,
    };
    let catalog_sha256 = write_pretty_json_noclobber(&catalog, output_path, "game asset catalog")?;

    Ok(GameAssetCatalogWriteReport {
        output: output_path.to_path_buf(),
        catalog_sha256,
        game_file_count: catalog.analysis.game_file_count,
        compile_lz_file_count: catalog.analysis.compile_lz_file_count,
        message_entry_count: catalog.analysis.message_entry_count,
    })
}

fn write_pretty_json_noclobber<T: Serialize>(
    value: &T,
    output_path: &Path,
    label: &str,
) -> Result<String> {
    ensure!(
        !output_path.exists(),
        "refusing to overwrite existing {label} {}",
        output_path.display()
    );
    let mut encoded = serde_json::to_vec_pretty(value)?;
    encoded.push(b'\n');
    let catalog_sha256 = sha256_hex(&encoded);

    let parent = output_path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)
        .with_context(|| format!("could not create catalog directory {}", parent.display()))?;
    let mut temporary = NamedTempFile::new_in(parent)
        .with_context(|| format!("could not stage {label} in {}", parent.display()))?;
    temporary
        .write_all(&encoded)
        .with_context(|| format!("could not write staged {label}"))?;
    temporary
        .as_file_mut()
        .sync_all()
        .with_context(|| format!("could not sync staged {label}"))?;
    temporary
        .persist_noclobber(output_path)
        .map_err(|error| error.error)
        .with_context(|| format!("could not publish {label} {}", output_path.display()))?;

    Ok(catalog_sha256)
}

fn consumer_evidence() -> Vec<ConsumerEvidence> {
    vec![
        ConsumerEvidence {
            program: "MADO456.COM",
            file_offset: 0x0e40,
            runtime_address: 0x0f40,
            observation: "selects MSG.DAT, MSGEV.DAT, or MSGnn.DAT from the filename table, loads it through DORI-BIOS, then requests decompression",
        },
        ConsumerEvidence {
            program: "MADO456.COM",
            file_offset: 0x1253,
            runtime_address: 0x1353,
            observation: "indexes one of two 256-word decoded message offset tables and resolves the selected record pointer",
        },
        ConsumerEvidence {
            program: "MADO456.COM",
            file_offset: 0x0723,
            runtime_address: 0x0823,
            observation: "interprets message controls, maps single-byte glyph codes, passes CP932 double-byte codes, and stops at 0xff",
        },
        ConsumerEvidence {
            program: "MADO456.COM",
            file_offset: FIRST_GLYPH_TABLE_FILE_OFFSET,
            runtime_address: FIRST_GLYPH_TABLE_FILE_OFFSET + COM_LOAD_ORIGIN,
            observation: "maps source bytes 0x20-0x7f to CP932 glyph codes",
        },
        ConsumerEvidence {
            program: "MADO456.COM",
            file_offset: SECOND_GLYPH_TABLE_FILE_OFFSET,
            runtime_address: SECOND_GLYPH_TABLE_FILE_OFFSET + COM_LOAD_ORIGIN,
            observation: "maps source bytes 0xa0-0xdf to CP932 glyph codes",
        },
        ConsumerEvidence {
            program: "OPENING.COM",
            file_offset: 0x18bc,
            runtime_address: 0x19bc,
            observation: "walks eight-byte inline-text descriptors, converts each pointed string to glyph data, and draws it at the descriptor coordinates",
        },
        ConsumerEvidence {
            program: "ENDING.COM",
            file_offset: 0x2f3b,
            runtime_address: 0x303b,
            observation: "walks eight-byte ending text descriptors and converts the pointed two-byte glyph strings before drawing them",
        },
        ConsumerEvidence {
            program: "ENDING.COM",
            file_offset: 0x2f9d,
            runtime_address: 0x309d,
            observation: "walks eight-byte staff-credit descriptors and draws each pointed string at the descriptor coordinates",
        },
    ]
}

fn find_unique_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    let mut offsets = haystack
        .windows(needle.len())
        .enumerate()
        .filter_map(|(offset, candidate)| (candidate == needle).then_some(offset));
    let first = offsets.next()?;
    offsets.next().is_none().then_some(first)
}

#[cfg(test)]
#[path = "static_analysis_tests.rs"]
mod tests;
