use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;

use anyhow::{Context, Result, bail, ensure};
use serde::Serialize;

use super::graphic_text_review::{GraphicTextReview, review_graphic_text_surfaces};
use super::graphic_text_usage::{
    GraphicTextUsageAnalysis, analyze_ending_graphic_text_usage, analyze_mdsc_graphic_text_usage,
    analyze_opening_graphic_text_usage,
};
use super::indexed_map_graphic_usage::{
    IndexedMapGraphicTextUsageAnalysis, analyze_mado456_map_graphic_text_usage,
};
use super::{
    COM_LOAD_ORIGIN, ENDING_SHA256, MADO456_SHA256, OPENING_SHA256, read_u16_le, require_file,
    require_sha256,
};
use crate::compile_lz::ExactCompileLzReport;
use crate::masked_tile::{
    ATLAS_DECODED_SIZE as MASKED_TILE_ATLAS_DECODED_SIZE,
    BYTES_PER_PLANE as MASKED_TILE_BYTES_PER_PLANE, BYTES_PER_TILE as MASKED_TILE_BYTES_PER_TILE,
    PLANE_COUNT as MASKED_TILE_PLANE_COUNT, TILE_COUNT as MASKED_TILE_COUNT,
    TILE_HEIGHT as MASKED_TILE_HEIGHT, TILE_WIDTH as MASKED_TILE_WIDTH,
    plane_range as masked_tile_plane_range,
};
use crate::source_cd::GameFile;

const OPENING_FILENAME_TABLE_FILE_OFFSET: usize = 0x4220;
const ENDING_FILENAME_TABLE_FILE_OFFSET: usize = 0x7320;
const OPENING_ASSET_LOADER_FILE_OFFSET: usize = 0x205d;
const ENDING_ASSET_LOADER_FILE_OFFSET: usize = 0x352e;
const OPENING_PLANAR_TRANSFER_FILE_OFFSET: usize = 0x1d16;
const OPENING_PLANAR_DESTINATION_TABLE_FILE_OFFSET: usize = 0x4218;
const OPENING_PLANAR_DESTINATION_SEGMENTS: [u16; 4] = [0xa800, 0xb000, 0xb800, 0xe000];
const OPENING_GRAPHIC_BUFFER_SIZE_TABLE_FILE_OFFSET: usize = 0x4206;
const ENDING_GRAPHIC_BUFFER_SIZE_TABLE_FILE_OFFSET: usize = 0x7306;
const GRAPHIC_BUFFER_SIZES: [u16; 9] = [
    0xa000, 0xa000, 0xa000, 0xa000, 0x7d00, 0x7d00, 0x7d00, 0x7d00, 0x8000,
];
const PLANAR_SCREEN_WIDTH: usize = 640;
const PLANAR_SCREEN_HEIGHT: usize = 400;
const PLANAR_SCREEN_BYTES_PER_PLANE: usize = PLANAR_SCREEN_WIDTH * PLANAR_SCREEN_HEIGHT / 8;
const OPENING_DIRECT_TILE_TRANSFER_FILE_OFFSET: usize = 0x2818;
const OPENING_MASKED_TILE_TRANSFER_FILE_OFFSET: usize = 0x2a18;
const ENDING_DIRECT_TILE_TRANSFER_FILE_OFFSET: usize = 0x3ce9;
const ENDING_MASKED_TILE_TRANSFER_FILE_OFFSET: usize = 0x3e7a;
const DIRECT_TILE_TRANSFER_SIZE: usize = 0x14a;
const MASKED_TILE_TRANSFER_SIZE: usize = 0x1b9;
const MADO456_TILE_FILENAME_TABLE_FILE_OFFSET: usize = 0x1902;
const MADO456_FILE_LOADER_FILE_OFFSET: usize = 0x0c80;
const MADO456_TILE_BUFFER_LOADER_FILE_OFFSET: usize = 0x0d12;
const MADO456_TILE_EXTRACTION_FILE_OFFSET: usize = 0x1f7d;
const MDSC_SHA256: &str = "ccabc705f253c56c57726f00471b4ce8db10bfbfaa19a08c34cfa77423e950cc";
const MDSC_BUFFER_ALLOCATION_FILE_OFFSET: usize = 0x004a;
const MDSC_INITIAL_ASSET_LOADER_FILE_OFFSET: usize = 0x0129;
const MDSC_CHARACTER_ASSET_LOADER_FILE_OFFSET: usize = 0x0187;
const MDSC_DIRECT_TILE_TRANSFER_FILE_OFFSET: usize = 0x11cb;
const MDSC_MASKED_TILE_TRANSFER_FILE_OFFSET: usize = 0x131b;
const EXPECTED_MASKED_TILE_ATLAS_FILE_COUNT: usize = 37;
const EXPECTED_CONSUMER_UNRESOLVED_ATLAS_COUNT: usize = 1;
const OPENING_CONFIGURATION_ASSET_LOAD_FILE_OFFSET: usize = 0x1de5;
const OPENING_CONFIGURATION_ASSET_LOAD_BYTES: [u8; 17] = [
    0x9c, 0x60, 0xbf, 0x06, 0x43, 0xbe, 0x28, 0x43, 0xb9, 0x03, 0x00, 0xe8, 0x6a, 0x02, 0x61, 0x9d,
    0xc3,
];

const MADO456_MASKED_TILE_FILENAME_REFERENCES: [(&str, usize); 19] = [
    ("MM_FDA.DAT", 0),
    ("MM_FDB.DAT", 1),
    ("MM_SR1A.DAT", 2),
    ("MM_SR1B.DAT", 3),
    ("MM_SR2A.DAT", 4),
    ("MM_SR2B.DAT", 5),
    ("MM_CK.DAT", 6),
    ("MM_CK.DAT", 7),
    ("MM_TWA.DAT", 8),
    ("MM_TWB.DAT", 9),
    ("MM_TWA.DAT", 10),
    ("MM_TWB.DAT", 11),
    ("MM_TWA.DAT", 12),
    ("MM_TWB.DAT", 13),
    ("MM_DK.DAT", 14),
    ("MM_DK.DAT", 15),
    ("ICON.DAT", 16),
    ("BEAM.DAT", 18),
    ("MAP_CHR.DAT", 26),
];

const MDSC_MASKED_TILE_FILENAME_REFERENCES: [(&str, usize); 5] = [
    ("FACE.DAT", 0x16bc),
    ("SELECT.DAT", 0x16c5),
    ("BG_S.DAT", 0x16d0),
    ("C_CHAR1.DAT", 0x16d9),
    ("C_CHAR2.DAT", 0x16e5),
];

const OPENING_FILENAME_TABLE_NAMES: [&str; 21] = [
    "OP_PG_S.DAT",
    "CURKUN.DAT",
    "FACE.DAT",
    "TITLE.DAT",
    "BG_S.DAT",
    "CFG_N.DAT",
    "CFG.DAT",
    "OP1.CNS",
    "OP2.CNS",
    "OP3.CNS",
    "OP4.CNS",
    "EC_1_S.DAT",
    "EC_2_S.DAT",
    "EC_3_S.DAT",
    "EC_4_S.DAT",
    "EC_5_S.DAT",
    "EC_6_S.DAT",
    "TITLE1.CNS",
    "TITLE2.CNS",
    "TITLE3.CNS",
    "TITLE4.CNS",
];

const ENDING_FILENAME_TABLE_NAMES: [&str; 14] = [
    "ED1_1.DAT",
    "ED1_2.DAT",
    "ED_2.DAT",
    "ED_3.DAT",
    "ED_4.DAT",
    "ED_5.DAT",
    "ED_6.DAT",
    "EC_1_S.DAT",
    "EC_2_S.DAT",
    "EC_3_S.DAT",
    "EC_4_S.DAT",
    "EC_5_S.DAT",
    "EC_6_S.DAT",
    "FIN.DAT",
];

pub(super) struct GraphicAnalysis {
    pub(super) program_filename_tables: Vec<ProgramFilenameTable>,
    pub(super) graphic_memory_layouts: Vec<GraphicMemoryLayout>,
    pub(super) planar_graphic_sets: Vec<PlanarGraphicSet>,
    pub(super) masked_tile_graphics: MaskedTileGraphicAnalysis,
}

#[derive(Debug, Serialize)]
pub(super) struct ProgramFilenameTable {
    program: &'static str,
    program_sha256: &'static str,
    pointer_table_file_offset: usize,
    pointer_table_runtime_address: usize,
    loader_file_offset: usize,
    loader_runtime_address: usize,
    entries: Vec<ProgramFilenameEntry>,
}

#[derive(Debug, Serialize)]
struct ProgramFilenameEntry {
    index: usize,
    name: String,
    filename_file_offset: usize,
    filename_runtime_address: usize,
}

#[derive(Debug, Serialize)]
pub(super) struct GraphicMemoryLayout {
    program: &'static str,
    size_table_file_offset: usize,
    size_table_runtime_address: usize,
    decoded_tile_buffer_count: usize,
    decoded_tile_bytes_per_buffer: usize,
    decoded_screen_plane_buffer_count: usize,
    decoded_screen_plane_bytes_per_buffer: usize,
    compressed_input_buffer_count: usize,
    compressed_input_bytes_per_buffer: usize,
}

#[derive(Debug, Serialize)]
pub(super) struct PlanarGraphicSet {
    id: &'static str,
    role: &'static str,
    files: Vec<String>,
    consumer_program: &'static str,
    filename_table_indexes: Vec<usize>,
    buffer_count: usize,
    decoded_bytes_per_buffer: usize,
    bits_per_buffer_pixel: usize,
    width: usize,
    height: usize,
    transfer_file_offset: usize,
    transfer_runtime_address: usize,
    destination_segment_table_file_offset: usize,
    destination_segments_hex: Vec<String>,
    buffer_roles: &'static str,
}

#[derive(Debug, Serialize)]
pub(super) struct MaskedTileGraphicAnalysis {
    decoded_bytes_per_atlas: usize,
    tile_count: usize,
    tile_width: usize,
    tile_height: usize,
    bytes_per_tile: usize,
    plane_count: usize,
    bytes_per_tile_plane: usize,
    plane_layout: &'static str,
    format_confirmed_atlas_count: usize,
    format_confirmation_basis: &'static str,
    consumer_confirmed_atlas_count: usize,
    consumer_confirmed_atlases: Vec<MaskedTileAtlas>,
    consumer_unresolved_atlas_count: usize,
    consumer_unresolved_atlases: Vec<String>,
    consumer_unresolved_assessments: Vec<ConsumerUnresolvedAtlasAssessment>,
    graphic_text_review: GraphicTextReview,
    graphic_text_usage: Vec<GraphicTextUsageAnalysis>,
    indexed_map_graphic_text_usage: IndexedMapGraphicTextUsageAnalysis,
    consumer_routines: Vec<TileConsumerRoutine>,
}

#[derive(Debug, Serialize)]
struct MaskedTileAtlas {
    name: String,
    consumers: Vec<AssetTableReference>,
}

#[derive(Debug, Serialize)]
struct AssetTableReference {
    program: &'static str,
    reference_kind: &'static str,
    reference_index: usize,
    filename_file_offset: usize,
    filename_runtime_address: usize,
}

#[derive(Debug, Serialize)]
struct ConsumerUnresolvedAtlasAssessment {
    name: &'static str,
    searched_program_count: usize,
    searched_programs: Vec<String>,
    case_insensitive_compact_filename_literal_count: usize,
    case_insensitive_padded_filename_literal_count: usize,
    opening_configuration_load_file_offset: usize,
    opening_configuration_load_runtime_address: usize,
    opening_configuration_filename_table_indexes: Vec<usize>,
    opening_configuration_asset_names: Vec<String>,
    status: &'static str,
}

#[derive(Debug, Serialize)]
struct TileConsumerRoutine {
    program: &'static str,
    file_offset: usize,
    runtime_address: usize,
    observation: &'static str,
}

pub(super) fn analyze_graphics(
    files: &[GameFile],
    compile_lz_reports: &BTreeMap<&str, &ExactCompileLzReport>,
) -> Result<GraphicAnalysis> {
    let opening_program = require_file(files, "OPENING.COM")?;
    require_sha256("OPENING.COM", &opening_program.bytes, OPENING_SHA256)?;
    let ending_program = require_file(files, "ENDING.COM")?;
    require_sha256("ENDING.COM", &ending_program.bytes, ENDING_SHA256)?;
    let main_program = require_file(files, "MADO456.COM")?;
    require_sha256("MADO456.COM", &main_program.bytes, MADO456_SHA256)?;
    let selection_program = require_file(files, "MDSC.COM")?;
    require_sha256("MDSC.COM", &selection_program.bytes, MDSC_SHA256)?;
    verify_shared_graphic_routines(opening_program, ending_program)?;

    let opening_filename_table = parse_program_filename_table(
        opening_program,
        OPENING_SHA256,
        OPENING_FILENAME_TABLE_FILE_OFFSET,
        OPENING_ASSET_LOADER_FILE_OFFSET,
        &OPENING_FILENAME_TABLE_NAMES,
    )?;
    let ending_filename_table = parse_program_filename_table(
        ending_program,
        ENDING_SHA256,
        ENDING_FILENAME_TABLE_FILE_OFFSET,
        ENDING_ASSET_LOADER_FILE_OFFSET,
        &ENDING_FILENAME_TABLE_NAMES,
    )?;

    let graphic_memory_layouts = vec![
        verify_graphic_memory_layout(
            opening_program,
            "OPENING.COM",
            OPENING_GRAPHIC_BUFFER_SIZE_TABLE_FILE_OFFSET,
        )?,
        verify_graphic_memory_layout(
            ending_program,
            "ENDING.COM",
            ENDING_GRAPHIC_BUFFER_SIZE_TABLE_FILE_OFFSET,
        )?,
    ];
    let planar_graphic_sets = vec![
        verify_planar_graphic_set(
            files,
            compile_lz_reports,
            &opening_filename_table,
            "opening_full_screen",
            "opening full-screen graphic",
            7..11,
        )?,
        verify_planar_graphic_set(
            files,
            compile_lz_reports,
            &opening_filename_table,
            "title_full_screen",
            "title full-screen graphic",
            17..21,
        )?,
    ];
    let masked_tile_graphics = classify_masked_tile_graphics(
        files,
        compile_lz_reports,
        main_program,
        selection_program,
        [&opening_filename_table, &ending_filename_table],
    )?;

    Ok(GraphicAnalysis {
        program_filename_tables: vec![opening_filename_table, ending_filename_table],
        graphic_memory_layouts,
        planar_graphic_sets,
        masked_tile_graphics,
    })
}

fn parse_program_filename_table(
    program: &GameFile,
    expected_sha256: &'static str,
    pointer_table_file_offset: usize,
    loader_file_offset: usize,
    expected_names: &[&str],
) -> Result<ProgramFilenameTable> {
    require_sha256(&program.display_name, &program.bytes, expected_sha256)?;
    let mut entries = Vec::with_capacity(expected_names.len());
    for (index, expected_name) in expected_names.iter().enumerate() {
        let pointer_offset = pointer_table_file_offset + index * 2;
        let filename_runtime_address = read_u16_le(&program.bytes, pointer_offset)? as usize;
        let filename_file_offset = filename_runtime_address
            .checked_sub(COM_LOAD_ORIGIN)
            .context("program filename pointer is below the COM load origin")?;
        let terminator = program.bytes[filename_file_offset..]
            .iter()
            .position(|byte| *byte == 0)
            .map(|relative| filename_file_offset + relative)
            .with_context(|| {
                format!(
                    "{} filename table entry {index} has no NUL terminator",
                    program.display_name
                )
            })?;
        let observed_name = std::str::from_utf8(&program.bytes[filename_file_offset..terminator])
            .with_context(|| {
            format!(
                "{} filename table entry {index} is not ASCII",
                program.display_name
            )
        })?;
        ensure!(
            observed_name.eq_ignore_ascii_case(expected_name),
            "{} filename table entry {index} changed: expected {expected_name}, got {observed_name}",
            program.display_name
        );
        entries.push(ProgramFilenameEntry {
            index,
            name: (*expected_name).to_owned(),
            filename_file_offset,
            filename_runtime_address,
        });
    }

    let (program_name, program_sha256) = match program.display_name.as_str() {
        "OPENING.COM" => ("OPENING.COM", OPENING_SHA256),
        "ENDING.COM" => ("ENDING.COM", ENDING_SHA256),
        _ => bail!(
            "unsupported program filename table {}",
            program.display_name
        ),
    };
    Ok(ProgramFilenameTable {
        program: program_name,
        program_sha256,
        pointer_table_file_offset,
        pointer_table_runtime_address: pointer_table_file_offset + COM_LOAD_ORIGIN,
        loader_file_offset,
        loader_runtime_address: loader_file_offset + COM_LOAD_ORIGIN,
        entries,
    })
}

fn verify_graphic_memory_layout(
    program: &GameFile,
    program_name: &'static str,
    size_table_file_offset: usize,
) -> Result<GraphicMemoryLayout> {
    for (index, expected_size) in GRAPHIC_BUFFER_SIZES.iter().enumerate() {
        let observed = read_u16_le(&program.bytes, size_table_file_offset + index * 2)?;
        ensure!(
            observed == *expected_size,
            "{program_name} graphic buffer size {index} changed: expected {expected_size:#06x}, got {observed:#06x}"
        );
    }

    Ok(GraphicMemoryLayout {
        program: program_name,
        size_table_file_offset,
        size_table_runtime_address: size_table_file_offset + COM_LOAD_ORIGIN,
        decoded_tile_buffer_count: 4,
        decoded_tile_bytes_per_buffer: MASKED_TILE_ATLAS_DECODED_SIZE,
        decoded_screen_plane_buffer_count: 4,
        decoded_screen_plane_bytes_per_buffer: PLANAR_SCREEN_BYTES_PER_PLANE,
        compressed_input_buffer_count: 1,
        compressed_input_bytes_per_buffer: 0x8000,
    })
}

fn verify_shared_graphic_routines(opening: &GameFile, ending: &GameFile) -> Result<()> {
    for (label, opening_offset, ending_offset, size) in [
        (
            "direct tile transfer",
            OPENING_DIRECT_TILE_TRANSFER_FILE_OFFSET,
            ENDING_DIRECT_TILE_TRANSFER_FILE_OFFSET,
            DIRECT_TILE_TRANSFER_SIZE,
        ),
        (
            "masked tile transfer",
            OPENING_MASKED_TILE_TRANSFER_FILE_OFFSET,
            ENDING_MASKED_TILE_TRANSFER_FILE_OFFSET,
            MASKED_TILE_TRANSFER_SIZE,
        ),
    ] {
        let opening_routine = opening
            .bytes
            .get(opening_offset..opening_offset + size)
            .with_context(|| format!("OPENING.COM {label} routine is truncated"))?;
        let ending_routine = ending
            .bytes
            .get(ending_offset..ending_offset + size)
            .with_context(|| format!("ENDING.COM {label} routine is truncated"))?;
        ensure!(
            opening_routine == ending_routine,
            "OPENING.COM and ENDING.COM {label} routines differ"
        );
    }
    Ok(())
}

fn verify_planar_graphic_set(
    files: &[GameFile],
    compile_lz_reports: &BTreeMap<&str, &ExactCompileLzReport>,
    filename_table: &ProgramFilenameTable,
    id: &'static str,
    role: &'static str,
    filename_table_indexes: Range<usize>,
) -> Result<PlanarGraphicSet> {
    ensure!(
        filename_table.program == "OPENING.COM",
        "planar screen sets require the OPENING.COM asset table"
    );
    let opening_program = require_file(files, "OPENING.COM")?;
    for (index, expected_segment) in OPENING_PLANAR_DESTINATION_SEGMENTS.iter().enumerate() {
        let observed = read_u16_le(
            &opening_program.bytes,
            OPENING_PLANAR_DESTINATION_TABLE_FILE_OFFSET + index * 2,
        )?;
        ensure!(
            observed == *expected_segment,
            "OPENING.COM planar destination segment {index} changed"
        );
    }
    let indexes: Vec<usize> = filename_table_indexes.collect();
    ensure!(indexes.len() == 4, "planar screen set must have four files");
    let mut names = Vec::with_capacity(indexes.len());
    for index in &indexes {
        let entry = filename_table
            .entries
            .get(*index)
            .with_context(|| format!("planar screen table index {index} is missing"))?;
        let file = require_file(files, &entry.name)?;
        let report = compile_lz_reports
            .get(file.display_name.as_str())
            .with_context(|| format!("{} is not exact Compile-LZ", file.display_name))?;
        ensure!(
            report.streams.len() == 1 && report.decoded_size == PLANAR_SCREEN_BYTES_PER_PLANE,
            "{} is not one full 640x400 bit plane",
            file.display_name
        );
        names.push(entry.name.clone());
    }

    Ok(PlanarGraphicSet {
        id,
        role,
        files: names,
        consumer_program: "OPENING.COM",
        filename_table_indexes: indexes,
        buffer_count: 4,
        decoded_bytes_per_buffer: PLANAR_SCREEN_BYTES_PER_PLANE,
        bits_per_buffer_pixel: 1,
        width: PLANAR_SCREEN_WIDTH,
        height: PLANAR_SCREEN_HEIGHT,
        transfer_file_offset: OPENING_PLANAR_TRANSFER_FILE_OFFSET,
        transfer_runtime_address: OPENING_PLANAR_TRANSFER_FILE_OFFSET + COM_LOAD_ORIGIN,
        destination_segment_table_file_offset: OPENING_PLANAR_DESTINATION_TABLE_FILE_OFFSET,
        destination_segments_hex: OPENING_PLANAR_DESTINATION_SEGMENTS
            .iter()
            .map(|segment| format!("{segment:04x}"))
            .collect(),
        buffer_roles: "A800/B000/B800/E000 screen planes; color order is unresolved",
    })
}

fn classify_masked_tile_graphics(
    files: &[GameFile],
    compile_lz_reports: &BTreeMap<&str, &ExactCompileLzReport>,
    main_program: &GameFile,
    selection_program: &GameFile,
    filename_tables: [&ProgramFilenameTable; 2],
) -> Result<MaskedTileGraphicAnalysis> {
    ensure!(
        masked_tile_plane_range(MASKED_TILE_COUNT - 1, MASKED_TILE_PLANE_COUNT - 1)?
            == (MASKED_TILE_ATLAS_DECODED_SIZE - MASKED_TILE_BYTES_PER_PLANE
                ..MASKED_TILE_ATLAS_DECODED_SIZE),
        "masked tile layout does not cover exactly one decoded atlas"
    );

    let mut confirmed = BTreeMap::<String, Vec<AssetTableReference>>::new();
    for table in filename_tables {
        for entry in &table.entries {
            let file = require_file(files, &entry.name)?;
            let Some(report) = compile_lz_reports.get(file.display_name.as_str()) else {
                continue;
            };
            if report.streams.len() == 1 && report.decoded_size == MASKED_TILE_ATLAS_DECODED_SIZE {
                confirmed
                    .entry(entry.name.clone())
                    .or_default()
                    .push(AssetTableReference {
                        program: table.program,
                        reference_kind: "pointer table",
                        reference_index: entry.index,
                        filename_file_offset: entry.filename_file_offset,
                        filename_runtime_address: entry.filename_runtime_address,
                    });
            }
        }
    }
    add_mado456_masked_tile_references(main_program, compile_lz_reports, &mut confirmed)?;
    add_mdsc_masked_tile_references(selection_program, compile_lz_reports, &mut confirmed)?;
    ensure!(
        confirmed.len() == EXPECTED_MASKED_TILE_ATLAS_FILE_COUNT,
        "expected {EXPECTED_MASKED_TILE_ATLAS_FILE_COUNT} program-consumed masked tile atlases, got {}",
        confirmed.len()
    );

    let confirmed_names: BTreeSet<&str> = confirmed.keys().map(String::as_str).collect();
    let mut consumer_unresolved_atlases = Vec::new();
    for file in files {
        let Some(report) = compile_lz_reports.get(file.display_name.as_str()) else {
            continue;
        };
        if report.streams.len() == 1
            && report.decoded_size == MASKED_TILE_ATLAS_DECODED_SIZE
            && !confirmed_names.contains(file.display_name.as_str())
        {
            consumer_unresolved_atlases.push(file.display_name.clone());
        }
    }
    consumer_unresolved_atlases.sort();
    ensure!(
        consumer_unresolved_atlases.len() == EXPECTED_CONSUMER_UNRESOLVED_ATLAS_COUNT,
        "expected {EXPECTED_CONSUMER_UNRESOLVED_ATLAS_COUNT} consumer-unresolved atlases, got {}",
        consumer_unresolved_atlases.len()
    );
    let unresolved_names: BTreeSet<&str> = consumer_unresolved_atlases
        .iter()
        .map(String::as_str)
        .collect();
    let opening_filename_table = filename_tables
        .iter()
        .copied()
        .find(|table| table.program == "OPENING.COM")
        .context("OPENING.COM filename table is missing")?;
    let consumer_unresolved_assessments = assess_cfg_s_consumer_search(
        files,
        require_file(files, "OPENING.COM")?,
        opening_filename_table,
        &unresolved_names,
    )?;
    let graphic_text_review = review_graphic_text_surfaces(&confirmed_names, &unresolved_names)?;
    let graphic_text_usage = vec![
        analyze_mdsc_graphic_text_usage(selection_program)?,
        analyze_opening_graphic_text_usage(require_file(files, "OPENING.COM")?)?,
        analyze_ending_graphic_text_usage(require_file(files, "ENDING.COM")?)?,
    ];
    let indexed_map_graphic_text_usage = analyze_mado456_map_graphic_text_usage(main_program)?;

    Ok(MaskedTileGraphicAnalysis {
        decoded_bytes_per_atlas: MASKED_TILE_ATLAS_DECODED_SIZE,
        tile_count: MASKED_TILE_COUNT,
        tile_width: MASKED_TILE_WIDTH,
        tile_height: MASKED_TILE_HEIGHT,
        bytes_per_tile: MASKED_TILE_BYTES_PER_TILE,
        plane_count: MASKED_TILE_PLANE_COUNT,
        bytes_per_tile_plane: MASKED_TILE_BYTES_PER_PLANE,
        plane_layout: "tile-interleaved: one GRCG mask/coverage plane followed by A800/B000/B800/E000 color-plane payloads; color order is unresolved",
        format_confirmed_atlas_count: confirmed.len() + consumer_unresolved_atlases.len(),
        format_confirmation_basis: "37 atlases have program consumer evidence; CFG_S.DAT additionally forms coherent configuration glyphs in decoded mask and color-plane renders on the same 16x16 grid",
        consumer_confirmed_atlas_count: confirmed.len(),
        consumer_confirmed_atlases: confirmed
            .into_iter()
            .map(|(name, consumers)| MaskedTileAtlas { name, consumers })
            .collect(),
        consumer_unresolved_atlas_count: consumer_unresolved_atlases.len(),
        consumer_unresolved_atlases,
        consumer_unresolved_assessments,
        graphic_text_review,
        graphic_text_usage,
        indexed_map_graphic_text_usage,
        consumer_routines: vec![
            TileConsumerRoutine {
                program: "MADO456.COM",
                file_offset: MADO456_FILE_LOADER_FILE_OFFSET,
                runtime_address: MADO456_FILE_LOADER_FILE_OFFSET + COM_LOAD_ORIGIN,
                observation: "reads a selected 13-byte padded filename through DORI-BIOS into the shared input buffer",
            },
            TileConsumerRoutine {
                program: "MADO456.COM",
                file_offset: MADO456_TILE_BUFFER_LOADER_FILE_OFFSET,
                runtime_address: MADO456_TILE_BUFFER_LOADER_FILE_OFFSET + COM_LOAD_ORIGIN,
                observation: "selects the fixed filename table and decompresses into tile buffers separated by 0xa00 paragraphs (40,960 bytes)",
            },
            TileConsumerRoutine {
                program: "MADO456.COM",
                file_offset: MADO456_TILE_EXTRACTION_FILE_OFFSET,
                runtime_address: MADO456_TILE_EXTRACTION_FILE_OFFSET + COM_LOAD_ORIGIN,
                observation: "computes tile_index * 0xa0, skips 0x20 mask bytes, and stages 0x80 color-plane bytes for drawing",
            },
            TileConsumerRoutine {
                program: "MDSC.COM",
                file_offset: MDSC_BUFFER_ALLOCATION_FILE_OFFSET,
                runtime_address: MDSC_BUFFER_ALLOCATION_FILE_OFFSET + COM_LOAD_ORIGIN,
                observation: "allocates four decoded asset segments separated by 0xa00 paragraphs (40,960 bytes)",
            },
            TileConsumerRoutine {
                program: "MDSC.COM",
                file_offset: MDSC_INITIAL_ASSET_LOADER_FILE_OFFSET,
                runtime_address: MDSC_INITIAL_ASSET_LOADER_FILE_OFFSET + COM_LOAD_ORIGIN,
                observation: "loads and decompresses FACE, SELECT, and BG_S into the first three 40,960-byte buffers",
            },
            TileConsumerRoutine {
                program: "MDSC.COM",
                file_offset: MDSC_CHARACTER_ASSET_LOADER_FILE_OFFSET,
                runtime_address: MDSC_CHARACTER_ASSET_LOADER_FILE_OFFSET + COM_LOAD_ORIGIN,
                observation: "loads and decompresses C_CHAR1 and C_CHAR2 into the first two 40,960-byte buffers",
            },
            TileConsumerRoutine {
                program: "MDSC.COM",
                file_offset: MDSC_DIRECT_TILE_TRANSFER_FILE_OFFSET,
                runtime_address: MDSC_DIRECT_TILE_TRANSFER_FILE_OFFSET + COM_LOAD_ORIGIN,
                observation: "computes tile_index * 160, skips the first plane, and copies four 16x16 color planes",
            },
            TileConsumerRoutine {
                program: "MDSC.COM",
                file_offset: MDSC_MASKED_TILE_TRANSFER_FILE_OFFSET,
                runtime_address: MDSC_MASKED_TILE_TRANSFER_FILE_OFFSET + COM_LOAD_ORIGIN,
                observation: "computes tile_index * 160 and consumes all five 32-byte planes through GRCG writes",
            },
            TileConsumerRoutine {
                program: "OPENING.COM",
                file_offset: OPENING_DIRECT_TILE_TRANSFER_FILE_OFFSET,
                runtime_address: OPENING_DIRECT_TILE_TRANSFER_FILE_OFFSET + COM_LOAD_ORIGIN,
                observation: "computes tile_index * 160, skips the first 32-byte plane, and copies four 32-byte 16x16 planes to A800/B000/B800/E000",
            },
            TileConsumerRoutine {
                program: "OPENING.COM",
                file_offset: OPENING_MASKED_TILE_TRANSFER_FILE_OFFSET,
                runtime_address: OPENING_MASKED_TILE_TRANSFER_FILE_OFFSET + COM_LOAD_ORIGIN,
                observation: "consumes all five consecutive 32-byte planes through GRCG writes, using the first plane before the four color payloads",
            },
            TileConsumerRoutine {
                program: "ENDING.COM",
                file_offset: ENDING_DIRECT_TILE_TRANSFER_FILE_OFFSET,
                runtime_address: ENDING_DIRECT_TILE_TRANSFER_FILE_OFFSET + COM_LOAD_ORIGIN,
                observation: "byte-identical direct four-color-plane tile transfer used by ending assets",
            },
            TileConsumerRoutine {
                program: "ENDING.COM",
                file_offset: ENDING_MASKED_TILE_TRANSFER_FILE_OFFSET,
                runtime_address: ENDING_MASKED_TILE_TRANSFER_FILE_OFFSET + COM_LOAD_ORIGIN,
                observation: "byte-identical five-plane GRCG masked tile transfer used by ending assets",
            },
        ],
    })
}

fn assess_cfg_s_consumer_search(
    files: &[GameFile],
    opening_program: &GameFile,
    opening_filename_table: &ProgramFilenameTable,
    unresolved_names: &BTreeSet<&str>,
) -> Result<Vec<ConsumerUnresolvedAtlasAssessment>> {
    ensure!(
        unresolved_names == &BTreeSet::from(["CFG_S.DAT"]),
        "consumer-unresolved atlas assessment requires only CFG_S.DAT"
    );
    let searched_programs: Vec<&GameFile> = files
        .iter()
        .filter(|file| file.display_name.ends_with(".COM") || file.display_name.ends_with(".BAT"))
        .collect();
    ensure!(
        searched_programs.len() == 9,
        "expected nine verified COM/BAT consumers, got {}",
        searched_programs.len()
    );
    let compact_literal_count = searched_programs
        .iter()
        .map(|file| ascii_case_insensitive_match_count(&file.bytes, b"CFG_S.DAT"))
        .sum();
    let padded_literal_count = searched_programs
        .iter()
        .map(|file| ascii_case_insensitive_match_count(&file.bytes, b"CFG_S   .DAT"))
        .sum();
    ensure!(
        compact_literal_count == 0 && padded_literal_count == 0,
        "CFG_S.DAT unexpectedly appears in a verified COM/BAT consumer"
    );

    let observed_configuration_load = opening_program
        .bytes
        .get(
            OPENING_CONFIGURATION_ASSET_LOAD_FILE_OFFSET
                ..OPENING_CONFIGURATION_ASSET_LOAD_FILE_OFFSET
                    + OPENING_CONFIGURATION_ASSET_LOAD_BYTES.len(),
        )
        .context("OPENING.COM configuration asset load is truncated")?;
    ensure!(
        observed_configuration_load == OPENING_CONFIGURATION_ASSET_LOAD_BYTES,
        "OPENING.COM configuration asset load changed"
    );
    let configuration_entries: Vec<&ProgramFilenameEntry> = opening_filename_table
        .entries
        .iter()
        .filter(|entry| (4..7).contains(&entry.index))
        .collect();
    ensure!(
        configuration_entries
            .iter()
            .map(|entry| entry.name.as_str())
            .eq(["BG_S.DAT", "CFG_N.DAT", "CFG.DAT"]),
        "OPENING.COM configuration asset range changed"
    );

    Ok(vec![ConsumerUnresolvedAtlasAssessment {
        name: "CFG_S.DAT",
        searched_program_count: searched_programs.len(),
        searched_programs: searched_programs
            .iter()
            .map(|file| file.display_name.clone())
            .collect(),
        case_insensitive_compact_filename_literal_count: compact_literal_count,
        case_insensitive_padded_filename_literal_count: padded_literal_count,
        opening_configuration_load_file_offset: OPENING_CONFIGURATION_ASSET_LOAD_FILE_OFFSET,
        opening_configuration_load_runtime_address: OPENING_CONFIGURATION_ASSET_LOAD_FILE_OFFSET
            + COM_LOAD_ORIGIN,
        opening_configuration_filename_table_indexes: configuration_entries
            .iter()
            .map(|entry| entry.index)
            .collect(),
        opening_configuration_asset_names: configuration_entries
            .iter()
            .map(|entry| entry.name.clone())
            .collect(),
        status: "no static reference was found in the verified COM/BAT consumers; unused, historical, or externally constructed-name status remains unresolved",
    }])
}

fn ascii_case_insensitive_match_count(haystack: &[u8], needle: &[u8]) -> usize {
    if needle.is_empty() {
        return 0;
    }
    haystack
        .windows(needle.len())
        .filter(|candidate| {
            candidate
                .iter()
                .zip(needle)
                .all(|(left, right)| left.eq_ignore_ascii_case(right))
        })
        .count()
}

fn add_mado456_masked_tile_references(
    program: &GameFile,
    compile_lz_reports: &BTreeMap<&str, &ExactCompileLzReport>,
    confirmed: &mut BTreeMap<String, Vec<AssetTableReference>>,
) -> Result<()> {
    for (expected_name, index) in MADO456_MASKED_TILE_FILENAME_REFERENCES {
        let filename_file_offset = MADO456_TILE_FILENAME_TABLE_FILE_OFFSET + index * 13;
        let observed_name = parse_padded_filename(&program.bytes, filename_file_offset)?;
        ensure!(
            observed_name == expected_name,
            "MADO456.COM filename table entry {index} changed: expected {expected_name}, got {observed_name}"
        );
        require_masked_tile_size(expected_name, compile_lz_reports)?;
        let references = confirmed.entry(expected_name.to_owned()).or_default();
        if !references.iter().any(|reference| {
            reference.program == "MADO456.COM" && reference.reference_index == index
        }) {
            references.push(AssetTableReference {
                program: "MADO456.COM",
                reference_kind: "fixed 13-byte filename table",
                reference_index: index,
                filename_file_offset,
                filename_runtime_address: filename_file_offset + COM_LOAD_ORIGIN,
            });
        }
    }
    Ok(())
}

fn add_mdsc_masked_tile_references(
    program: &GameFile,
    compile_lz_reports: &BTreeMap<&str, &ExactCompileLzReport>,
    confirmed: &mut BTreeMap<String, Vec<AssetTableReference>>,
) -> Result<()> {
    for (reference_index, (expected_name, filename_file_offset)) in
        MDSC_MASKED_TILE_FILENAME_REFERENCES.into_iter().enumerate()
    {
        let expected_literal = expected_name.to_ascii_lowercase();
        let observed = program
            .bytes
            .get(filename_file_offset..filename_file_offset + expected_literal.len() + 1)
            .with_context(|| format!("MDSC.COM {expected_name} filename literal is truncated"))?;
        ensure!(
            observed[..expected_literal.len()] == *expected_literal.as_bytes()
                && observed[expected_literal.len()] == 0,
            "MDSC.COM {expected_name} filename literal changed"
        );
        require_masked_tile_size(expected_name, compile_lz_reports)?;
        confirmed
            .entry(expected_name.to_owned())
            .or_default()
            .push(AssetTableReference {
                program: "MDSC.COM",
                reference_kind: "literal load list",
                reference_index,
                filename_file_offset,
                filename_runtime_address: filename_file_offset + COM_LOAD_ORIGIN,
            });
    }
    Ok(())
}

fn require_masked_tile_size(
    name: &str,
    compile_lz_reports: &BTreeMap<&str, &ExactCompileLzReport>,
) -> Result<()> {
    let report = compile_lz_reports
        .get(name)
        .with_context(|| format!("{name} is not exact Compile-LZ"))?;
    ensure!(
        report.streams.len() == 1 && report.decoded_size == MASKED_TILE_ATLAS_DECODED_SIZE,
        "{name} is not one {MASKED_TILE_ATLAS_DECODED_SIZE}-byte masked tile atlas"
    );
    Ok(())
}

fn parse_padded_filename(bytes: &[u8], file_offset: usize) -> Result<String> {
    let record = bytes
        .get(file_offset..file_offset + 13)
        .with_context(|| format!("padded filename at file offset {file_offset:#x} is truncated"))?;
    ensure!(
        record[8] == b'.' && record[12] == 0,
        "padded filename at file offset {file_offset:#x} has invalid separators"
    );
    let stem = std::str::from_utf8(&record[..8])
        .context("padded filename stem is not ASCII")?
        .trim_end_matches(' ');
    let extension =
        std::str::from_utf8(&record[9..12]).context("padded filename extension is not ASCII")?;
    Ok(format!("{stem}.{extension}"))
}

#[cfg(test)]
#[path = "graphic_analysis_tests.rs"]
mod tests;
