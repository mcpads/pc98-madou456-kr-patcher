use std::collections::BTreeSet;

use anyhow::{Context, Result, ensure};
use serde::Serialize;

use super::COM_LOAD_ORIGIN;
use super::graphic_text_usage::hex_tile_index_ranges;
use crate::source_cd::GameFile;

const MADO456_TILE_BUFFER_LOADER_FILE_OFFSET: usize = 0x0d12;
const MADO456_MAP_TILE_DRAW_FILE_OFFSET: usize = 0xbe33;
const MADO456_MAP_TILE_DRAW_RUNTIME_ADDRESS: usize =
    MADO456_MAP_TILE_DRAW_FILE_OFFSET + COM_LOAD_ORIGIN;
const MADO456_MAP_TILE_SEGMENT_SLOT_RUNTIME_ADDRESS: usize = 0xc217;
const MADO456_MAP_STATE_COUNT: usize = 11;
const MADO456_MAP_UNIQUE_DESCRIPTOR_COUNT: usize = 21;
const MADO456_MAP_UNIQUE_TILE_COUNT: usize = 67;

const MADO456_MAP_LOAD_SPECS: [MapAssetLoadSpec; 2] = [
    MapAssetLoadSpec {
        runtime_address: 0xba52,
        expected_bytes: &[0xb0, 0x1a, 0xb4, 0x04, 0xe8, 0xb9, 0x53],
    },
    MapAssetLoadSpec {
        runtime_address: 0xba7d,
        expected_bytes: &[0xb0, 0x1a, 0xb4, 0x04, 0xe8, 0x8e, 0x53],
    },
];

const MADO456_MAP_SELECTOR_TABLE_SPECS: [MapDescriptorSelectorTableSpec; 4] = [
    MapDescriptorSelectorTableSpec {
        role: "primary map-state graphics",
        runtime_address: 0xbbc5,
        record_size: 12,
    },
    MapDescriptorSelectorTableSpec {
        role: "secondary map-state graphics",
        runtime_address: 0xbc6e,
        record_size: 12,
    },
    MapDescriptorSelectorTableSpec {
        role: "map-state overlay graphics",
        runtime_address: 0xbd17,
        record_size: 10,
    },
    MapDescriptorSelectorTableSpec {
        role: "tertiary map-state graphics",
        runtime_address: 0xbdaa,
        record_size: 14,
    },
];

const MADO456_MAP_DIRECT_DESCRIPTOR_RUNTIME_ADDRESSES: [usize; 10] = [
    0xc095, 0xc179, 0xc1bd, 0xc1c9, 0xc1d5, 0xc1e1, 0xc1ed, 0xc1f3, 0xc1f9, 0xc20d,
];

struct MapAssetLoadSpec {
    runtime_address: usize,
    expected_bytes: &'static [u8],
}

struct MapDescriptorSelectorTableSpec {
    role: &'static str,
    runtime_address: usize,
    record_size: usize,
}

#[derive(Debug, Serialize)]
pub(super) struct IndexedMapGraphicTextUsageAnalysis {
    consumer_program: &'static str,
    asset_name: &'static str,
    asset_filename_table_index: usize,
    loader_file_offset: usize,
    loader_runtime_address: usize,
    asset_segment_slot_runtime_address: usize,
    draw_routine_file_offset: usize,
    draw_routine_runtime_address: usize,
    descriptor_format: &'static str,
    load_sites: Vec<MapAssetLoadSite>,
    selector_tables: Vec<MapDescriptorSelectorTable>,
    directly_referenced_descriptor_runtime_addresses: Vec<usize>,
    unique_descriptor_count: usize,
    unique_tile_count: usize,
    tile_index_ranges_hex: Vec<String>,
    descriptors: Vec<IndexedTileMapDescriptorUsage>,
}

#[derive(Debug, Serialize)]
struct MapAssetLoadSite {
    file_offset: usize,
    runtime_address: usize,
}

#[derive(Debug, Serialize)]
struct MapDescriptorSelectorTable {
    role: &'static str,
    file_offset: usize,
    runtime_address: usize,
    state_count: usize,
    record_size: usize,
    descriptor_reference_count: usize,
    unique_descriptor_count: usize,
    state_descriptor_counts: Vec<usize>,
}

#[derive(Debug, Serialize)]
struct IndexedTileMapDescriptorUsage {
    descriptor_file_offset: usize,
    descriptor_runtime_address: usize,
    width_tiles: usize,
    height_tiles: usize,
    cell_count: usize,
    unique_tile_count: usize,
    tile_index_ranges_hex: Vec<String>,
    cell_high_byte_ranges_hex: Vec<String>,
}

struct ParsedIndexedTileMapDescriptor {
    width: usize,
    height: usize,
    tile_indexes: BTreeSet<u8>,
    cell_high_bytes: BTreeSet<u8>,
}

pub(super) fn analyze_mado456_map_graphic_text_usage(
    main_program: &GameFile,
) -> Result<IndexedMapGraphicTextUsageAnalysis> {
    ensure!(
        main_program.display_name == "MADO456.COM",
        "indexed map graphic text usage requires MADO456.COM"
    );

    let mut load_sites = Vec::new();
    for spec in MADO456_MAP_LOAD_SPECS {
        let file_offset = runtime_to_file_offset(spec.runtime_address, "MAP_CHR.DAT load site")?;
        let observed = main_program
            .bytes
            .get(file_offset..file_offset + spec.expected_bytes.len())
            .context("MAP_CHR.DAT load site is truncated")?;
        ensure!(
            observed == spec.expected_bytes,
            "MAP_CHR.DAT load site at {:#x} changed",
            spec.runtime_address
        );
        load_sites.push(MapAssetLoadSite {
            file_offset,
            runtime_address: spec.runtime_address,
        });
    }

    let mut descriptor_runtime_addresses =
        BTreeSet::from(MADO456_MAP_DIRECT_DESCRIPTOR_RUNTIME_ADDRESSES);
    let mut selector_tables = Vec::new();
    for spec in MADO456_MAP_SELECTOR_TABLE_SPECS {
        let records = parse_map_descriptor_selector_table(
            &main_program.bytes,
            spec.runtime_address,
            spec.record_size,
            MADO456_MAP_STATE_COUNT,
        )?;
        let mut table_descriptor_runtime_addresses = BTreeSet::new();
        let mut descriptor_reference_count = 0usize;
        let mut state_descriptor_counts = Vec::with_capacity(records.len());
        for record in records {
            descriptor_reference_count += record.len();
            state_descriptor_counts.push(record.len());
            table_descriptor_runtime_addresses.extend(record);
        }
        descriptor_runtime_addresses.extend(&table_descriptor_runtime_addresses);
        selector_tables.push(MapDescriptorSelectorTable {
            role: spec.role,
            file_offset: runtime_to_file_offset(
                spec.runtime_address,
                "map descriptor selector table",
            )?,
            runtime_address: spec.runtime_address,
            state_count: MADO456_MAP_STATE_COUNT,
            record_size: spec.record_size,
            descriptor_reference_count,
            unique_descriptor_count: table_descriptor_runtime_addresses.len(),
            state_descriptor_counts,
        });
    }
    ensure!(
        descriptor_runtime_addresses.len() == MADO456_MAP_UNIQUE_DESCRIPTOR_COUNT,
        "expected {MADO456_MAP_UNIQUE_DESCRIPTOR_COUNT} MAP_CHR.DAT descriptors, got {}",
        descriptor_runtime_addresses.len()
    );

    let mut descriptors = Vec::new();
    let mut tile_indexes = BTreeSet::new();
    for runtime_address in descriptor_runtime_addresses {
        let file_offset = runtime_to_file_offset(runtime_address, "MAP_CHR.DAT descriptor")?;
        let descriptor = parse_indexed_tile_map_descriptor(&main_program.bytes, file_offset)?;
        tile_indexes.extend(&descriptor.tile_indexes);
        descriptors.push(IndexedTileMapDescriptorUsage {
            descriptor_file_offset: file_offset,
            descriptor_runtime_address: runtime_address,
            width_tiles: descriptor.width,
            height_tiles: descriptor.height,
            cell_count: descriptor.width * descriptor.height,
            unique_tile_count: descriptor.tile_indexes.len(),
            tile_index_ranges_hex: hex_tile_index_ranges(&descriptor.tile_indexes),
            cell_high_byte_ranges_hex: hex_tile_index_ranges(&descriptor.cell_high_bytes),
        });
    }
    ensure!(
        tile_indexes.len() == MADO456_MAP_UNIQUE_TILE_COUNT,
        "expected {MADO456_MAP_UNIQUE_TILE_COUNT} MAP_CHR.DAT tiles, got {}",
        tile_indexes.len()
    );

    Ok(IndexedMapGraphicTextUsageAnalysis {
        consumer_program: "MADO456.COM",
        asset_name: "MAP_CHR.DAT",
        asset_filename_table_index: 26,
        loader_file_offset: MADO456_TILE_BUFFER_LOADER_FILE_OFFSET,
        loader_runtime_address: MADO456_TILE_BUFFER_LOADER_FILE_OFFSET + COM_LOAD_ORIGIN,
        asset_segment_slot_runtime_address: MADO456_MAP_TILE_SEGMENT_SLOT_RUNTIME_ADDRESS,
        draw_routine_file_offset: MADO456_MAP_TILE_DRAW_FILE_OFFSET,
        draw_routine_runtime_address: MADO456_MAP_TILE_DRAW_RUNTIME_ADDRESS,
        descriptor_format: "little-endian 16-bit width and height followed by row-major 16-bit cells; the low byte selects a tile and the high byte has values 0 or 1",
        load_sites,
        selector_tables,
        directly_referenced_descriptor_runtime_addresses:
            MADO456_MAP_DIRECT_DESCRIPTOR_RUNTIME_ADDRESSES.to_vec(),
        unique_descriptor_count: descriptors.len(),
        unique_tile_count: tile_indexes.len(),
        tile_index_ranges_hex: hex_tile_index_ranges(&tile_indexes),
        descriptors,
    })
}

fn parse_indexed_tile_map_descriptor(
    bytes: &[u8],
    file_offset: usize,
) -> Result<ParsedIndexedTileMapDescriptor> {
    let width = read_u16(bytes, file_offset, "indexed tile-map width")? as usize;
    let height = read_u16(bytes, file_offset + 2, "indexed tile-map height")? as usize;
    ensure!(
        width > 0 && height > 0,
        "indexed tile-map dimensions must be nonzero"
    );
    let cell_count = width
        .checked_mul(height)
        .context("indexed tile-map dimensions overflow")?;
    let cells_file_offset = file_offset + 4;
    let cells_byte_count = cell_count
        .checked_mul(2)
        .context("indexed tile-map byte count overflow")?;
    let cells = bytes
        .get(cells_file_offset..cells_file_offset + cells_byte_count)
        .context("indexed tile-map cells are truncated")?;
    let mut tile_indexes = BTreeSet::new();
    let mut cell_high_bytes = BTreeSet::new();
    let (cells, remainder) = cells.as_chunks::<2>();
    ensure!(remainder.is_empty(), "indexed tile-map has a partial cell");
    for cell in cells {
        tile_indexes.insert(cell[0]);
        cell_high_bytes.insert(cell[1]);
    }
    Ok(ParsedIndexedTileMapDescriptor {
        width,
        height,
        tile_indexes,
        cell_high_bytes,
    })
}

fn parse_map_descriptor_selector_table(
    bytes: &[u8],
    table_runtime_address: usize,
    record_size: usize,
    state_count: usize,
) -> Result<Vec<Vec<usize>>> {
    ensure!(record_size >= 2, "map selector records are too small");
    let table_file_offset =
        runtime_to_file_offset(table_runtime_address, "map descriptor selector table")?;
    let table_byte_count = record_size
        .checked_mul(state_count)
        .context("map selector table size overflow")?;
    let table = bytes
        .get(table_file_offset..table_file_offset + table_byte_count)
        .context("map descriptor selector table is truncated")?;
    let table_end = table_file_offset + table.len();

    let mut records = Vec::with_capacity(state_count);
    for state_index in 0..state_count {
        let record_start = table_file_offset + state_index * record_size;
        let mut cursor = record_start;
        let mut descriptor_runtime_addresses = Vec::new();
        loop {
            ensure!(
                cursor + 2 <= table_end,
                "map selector state {state_index} has no terminating descriptor word"
            );
            let descriptor_runtime_address =
                read_u16(bytes, cursor, "map descriptor selector")? as usize;
            cursor += 2;
            if descriptor_runtime_address == u16::MAX as usize {
                break;
            }
            ensure!(
                cursor + 2 <= table_end,
                "map selector state {state_index} has no destination word"
            );
            let _destination = read_u16(bytes, cursor, "map descriptor destination")?;
            cursor += 2;
            descriptor_runtime_addresses.push(descriptor_runtime_address);
        }
        records.push(descriptor_runtime_addresses);
    }
    Ok(records)
}

fn runtime_to_file_offset(runtime_address: usize, label: &str) -> Result<usize> {
    runtime_address
        .checked_sub(COM_LOAD_ORIGIN)
        .with_context(|| format!("{label} is below the COM load origin"))
}

fn read_u16(bytes: &[u8], file_offset: usize, label: &str) -> Result<u16> {
    let value = bytes
        .get(file_offset..file_offset + 2)
        .with_context(|| format!("{label} at file offset {file_offset:#x} is truncated"))?;
    Ok(u16::from_le_bytes([value[0], value[1]]))
}

#[cfg(test)]
#[path = "indexed_map_graphic_usage_tests.rs"]
mod tests;
