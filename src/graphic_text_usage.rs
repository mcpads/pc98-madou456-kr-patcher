use std::collections::BTreeSet;

use anyhow::{Context, Result, ensure};
use serde::Serialize;

use crate::source_cd::GameFile;

use super::COM_LOAD_ORIGIN;

const MDSC_MASKED_TILE_DRAW_FILE_OFFSET: usize = 0x131b;
const MDSC_MASKED_TILE_DRAW_RUNTIME_ADDRESS: usize =
    MDSC_MASKED_TILE_DRAW_FILE_OFFSET + COM_LOAD_ORIGIN;
const OPENING_DIRECT_TILE_DRAW_FILE_OFFSET: usize = 0x2818;
const OPENING_DIRECT_TILE_DRAW_RUNTIME_ADDRESS: usize =
    OPENING_DIRECT_TILE_DRAW_FILE_OFFSET + COM_LOAD_ORIGIN;
const ENDING_DIRECT_TILE_DRAW_FILE_OFFSET: usize = 0x3ce9;
const ENDING_DIRECT_TILE_DRAW_RUNTIME_ADDRESS: usize =
    ENDING_DIRECT_TILE_DRAW_FILE_OFFSET + COM_LOAD_ORIGIN;

const MDSC_TEXT_USAGE_SPECS: [GraphicTextUsageSpec; 3] = [
    GraphicTextUsageSpec {
        asset_name: "SELECT.DAT",
        observed_role: "selection frame and player label",
        asset_segment_runtime_address: 0x17f3,
        descriptor_runtime_address: 0x250e,
        call_site_runtime_address: 0x02ec,
        expected_call_site_bytes: &[
            0xbb, 0x0e, 0x25, 0x2e, 0x8e, 0x1e, 0xf3, 0x17, 0x2e, 0x8b, 0x3e, 0x19, 0x18, 0xe8,
            0x1f, 0x11,
        ],
    },
    GraphicTextUsageSpec {
        asset_name: "SELECT.DAT",
        observed_role: "team-selection prompt",
        asset_segment_runtime_address: 0x17f3,
        descriptor_runtime_address: 0x2ad4,
        call_site_runtime_address: 0x02fc,
        expected_call_site_bytes: &[
            0xbb, 0xd4, 0x2a, 0x2e, 0x8e, 0x1e, 0xf3, 0x17, 0xbf, 0x08, 0x37, 0xe8, 0x11, 0x11,
        ],
    },
    GraphicTextUsageSpec {
        asset_name: "C_CHAR1.DAT",
        observed_role: "team-selection prompt and player labels",
        asset_segment_runtime_address: 0x17f1,
        descriptor_runtime_address: 0x2b36,
        call_site_runtime_address: 0x03ed,
        expected_call_site_bytes: &[
            0xbb, 0x36, 0x2b, 0x2e, 0x8e, 0x1e, 0xf1, 0x17, 0xbf, 0x0c, 0x0a, 0xe8, 0x20, 0x10,
        ],
    },
];

const OPENING_TEXT_USAGE_SPECS: [GraphicTextUsageSpec; 12] = [
    GraphicTextUsageSpec {
        asset_name: "TITLE.DAT",
        observed_role: "game-start label",
        asset_segment_runtime_address: 0x430c,
        descriptor_runtime_address: 0x3b44,
        call_site_runtime_address: 0x071f,
        expected_call_site_bytes: &[
            0x2e, 0xa1, 0x0c, 0x43, 0x8e, 0xd8, 0xbf, 0x1a, 0x55, 0xbb, 0x44, 0x3b, 0xe8, 0xea,
            0x21,
        ],
    },
    GraphicTextUsageSpec {
        asset_name: "TITLE.DAT",
        observed_role: "configuration label",
        asset_segment_runtime_address: 0x430c,
        descriptor_runtime_address: 0x3b5e,
        call_site_runtime_address: 0x06eb,
        expected_call_site_bytes: &[0xbf, 0x1a, 0x5f, 0xbb, 0x5e, 0x3b, 0xe8, 0x24, 0x22],
    },
    GraphicTextUsageSpec {
        asset_name: "TITLE.DAT",
        observed_role: "game-start selected-state label",
        asset_segment_runtime_address: 0x430c,
        descriptor_runtime_address: 0x3b70,
        call_site_runtime_address: 0x06dd,
        expected_call_site_bytes: &[
            0xa1, 0x0c, 0x43, 0x8e, 0xd8, 0xbf, 0x1a, 0x55, 0xbb, 0x70, 0x3b, 0xe8, 0x2d, 0x22,
        ],
    },
    GraphicTextUsageSpec {
        asset_name: "TITLE.DAT",
        observed_role: "configuration selected-state label",
        asset_segment_runtime_address: 0x430c,
        descriptor_runtime_address: 0x3b8a,
        call_site_runtime_address: 0x072e,
        expected_call_site_bytes: &[0xbf, 0x1a, 0x5f, 0xbb, 0x8a, 0x3b, 0xe8, 0xe1, 0x21],
    },
    GraphicTextUsageSpec {
        asset_name: "CFG_N.DAT",
        observed_role: "configuration glyph block 0",
        asset_segment_runtime_address: 0x4308,
        descriptor_runtime_address: 0x3a14,
        call_site_runtime_address: 0x0ac9,
        expected_call_site_bytes: &[
            0xbb, 0x14, 0x3a, 0xbf, 0x18, 0x28, 0x2e, 0xa1, 0x08, 0x43, 0x8e, 0xd8, 0xe8, 0x40,
            0x1e,
        ],
    },
    GraphicTextUsageSpec {
        asset_name: "CFG_N.DAT",
        observed_role: "configuration glyph block 1",
        asset_segment_runtime_address: 0x4308,
        descriptor_runtime_address: 0x3a3a,
        call_site_runtime_address: 0x0b2b,
        expected_call_site_bytes: &[
            0xbb, 0x3a, 0x3a, 0xbf, 0x2c, 0x28, 0x2e, 0xa1, 0x08, 0x43, 0x8e, 0xd8, 0xe8, 0xde,
            0x1d,
        ],
    },
    GraphicTextUsageSpec {
        asset_name: "CFG.DAT",
        observed_role: "configuration glyph block 0",
        asset_segment_runtime_address: 0x430a,
        descriptor_runtime_address: 0x3a60,
        call_site_runtime_address: 0x0c21,
        expected_call_site_bytes: &[
            0xbb, 0x60, 0x3a, 0xbf, 0x1a, 0x05, 0x2e, 0xa1, 0x0a, 0x43, 0x8e, 0xd8, 0xe8, 0xe8,
            0x1c,
        ],
    },
    GraphicTextUsageSpec {
        asset_name: "CFG.DAT",
        observed_role: "configuration glyph block 1",
        asset_segment_runtime_address: 0x430a,
        descriptor_runtime_address: 0x3a86,
        call_site_runtime_address: 0x0cc3,
        expected_call_site_bytes: &[
            0xbb, 0x86, 0x3a, 0xbf, 0x2a, 0x05, 0x2e, 0xa1, 0x0a, 0x43, 0x8e, 0xd8, 0xe8, 0x46,
            0x1c,
        ],
    },
    GraphicTextUsageSpec {
        asset_name: "CFG.DAT",
        observed_role: "configuration glyph block 2",
        asset_segment_runtime_address: 0x430a,
        descriptor_runtime_address: 0x3aac,
        call_site_runtime_address: 0x0d65,
        expected_call_site_bytes: &[
            0xbb, 0xac, 0x3a, 0xbf, 0x0b, 0x0a, 0x2e, 0xa1, 0x0a, 0x43, 0x8e, 0xd8, 0xe8, 0xa4,
            0x1b,
        ],
    },
    GraphicTextUsageSpec {
        asset_name: "CFG.DAT",
        observed_role: "configuration glyph block 3",
        asset_segment_runtime_address: 0x430a,
        descriptor_runtime_address: 0x3ad2,
        call_site_runtime_address: 0x0e0d,
        expected_call_site_bytes: &[
            0xbb, 0xd2, 0x3a, 0xbf, 0x3a, 0x0a, 0x2e, 0xa1, 0x0a, 0x43, 0x8e, 0xd8, 0xe8, 0xfc,
            0x1a,
        ],
    },
    GraphicTextUsageSpec {
        asset_name: "CFG.DAT",
        observed_role: "configuration glyph block 4",
        asset_segment_runtime_address: 0x430a,
        descriptor_runtime_address: 0x3af8,
        call_site_runtime_address: 0x0eaf,
        expected_call_site_bytes: &[
            0xbb, 0xf8, 0x3a, 0xbf, 0x04, 0x2d, 0x2e, 0xa1, 0x0a, 0x43, 0x8e, 0xd8, 0xe8, 0x5a,
            0x1a,
        ],
    },
    GraphicTextUsageSpec {
        asset_name: "CFG.DAT",
        observed_role: "configuration glyph block 5",
        asset_segment_runtime_address: 0x430a,
        descriptor_runtime_address: 0x3b1e,
        call_site_runtime_address: 0x0f51,
        expected_call_site_bytes: &[
            0xbb, 0x1e, 0x3b, 0xbf, 0x41, 0x2d, 0x2e, 0xa1, 0x0a, 0x43, 0x8e, 0xd8, 0xe8, 0xb8,
            0x19,
        ],
    },
];

const ENDING_TEXT_USAGE_SPECS: [GraphicTextUsageSpec; 4] = [
    GraphicTextUsageSpec {
        asset_name: "FIN.DAT",
        observed_role: "ending copyright credit row",
        asset_segment_runtime_address: 0x7406,
        descriptor_runtime_address: 0x4c25,
        call_site_runtime_address: 0x2ea8,
        expected_call_site_bytes: &[
            0xa1, 0x06, 0x74, 0x8e, 0xd8, 0xbb, 0x25, 0x4c, 0xbf, 0x1a, 0x3c, 0xe8, 0x33, 0x0f,
        ],
    },
    GraphicTextUsageSpec {
        asset_name: "FIN.DAT",
        observed_role: "ending music credit row",
        asset_segment_runtime_address: 0x7406,
        descriptor_runtime_address: 0x4c35,
        call_site_runtime_address: 0x2eb6,
        expected_call_site_bytes: &[0xbb, 0x35, 0x4c, 0xbf, 0x18, 0x46, 0xe8, 0x2a, 0x0f],
    },
    GraphicTextUsageSpec {
        asset_name: "FIN.DAT",
        observed_role: "large ending title graphic",
        asset_segment_runtime_address: 0x7406,
        descriptor_runtime_address: 0x4bdd,
        call_site_runtime_address: 0x2f16,
        expected_call_site_bytes: &[
            0xa1, 0x06, 0x74, 0x8e, 0xd8, 0xbb, 0xdd, 0x4b, 0xbf, 0x32, 0x55, 0xe8, 0xc5, 0x0e,
        ],
    },
    GraphicTextUsageSpec {
        asset_name: "FIN.DAT",
        observed_role: "large ending message graphic",
        asset_segment_runtime_address: 0x7406,
        descriptor_runtime_address: 0x4c47,
        call_site_runtime_address: 0x2f94,
        expected_call_site_bytes: &[
            0xa1, 0x06, 0x74, 0x8e, 0xd8, 0xbb, 0x47, 0x4c, 0xbf, 0x52, 0x2e, 0xe8, 0x47, 0x0e,
        ],
    },
];

struct GraphicTextUsageSpec {
    asset_name: &'static str,
    observed_role: &'static str,
    asset_segment_runtime_address: usize,
    descriptor_runtime_address: usize,
    call_site_runtime_address: usize,
    expected_call_site_bytes: &'static [u8],
}

struct GraphicTextProgramSpec {
    program_name: &'static str,
    draw_routine_file_offset: usize,
    draw_routine_runtime_address: usize,
    descriptor_format: &'static str,
    asset_names: &'static [&'static str],
    usage_specs: &'static [GraphicTextUsageSpec],
    transparent_tile_index: Option<u8>,
}

#[derive(Debug, Serialize)]
pub(super) struct GraphicTextUsageAnalysis {
    consumer_program: &'static str,
    draw_routine_file_offset: usize,
    draw_routine_runtime_address: usize,
    descriptor_format: &'static str,
    asset_count: usize,
    assets: Vec<GraphicTextAssetUsage>,
}

#[derive(Debug, Serialize)]
struct GraphicTextAssetUsage {
    name: &'static str,
    asset_segment_runtime_address: usize,
    descriptor_count: usize,
    unique_tile_count: usize,
    tile_index_ranges_hex: Vec<String>,
    descriptors: Vec<TileMapDescriptorUsage>,
}

#[derive(Debug, Serialize)]
struct TileMapDescriptorUsage {
    observed_role: &'static str,
    call_site_file_offset: usize,
    call_site_runtime_address: usize,
    descriptor_file_offset: usize,
    descriptor_runtime_address: usize,
    width_tiles: usize,
    height_tiles: usize,
    cell_count: usize,
    referenced_cell_count: usize,
    transparent_cell_count: usize,
    unique_tile_count: usize,
    tile_index_ranges_hex: Vec<String>,
}

struct ParsedTileMapDescriptor {
    width: usize,
    height: usize,
    referenced_cell_count: usize,
    transparent_cell_count: usize,
    tile_indexes: BTreeSet<u8>,
}

pub(super) fn analyze_mdsc_graphic_text_usage(
    selection_program: &GameFile,
) -> Result<GraphicTextUsageAnalysis> {
    ensure!(
        selection_program.display_name == "MDSC.COM",
        "graphic text usage requires MDSC.COM"
    );
    analyze_program_graphic_text_usage(
        selection_program,
        &GraphicTextProgramSpec {
            program_name: "MDSC.COM",
            draw_routine_file_offset: MDSC_MASKED_TILE_DRAW_FILE_OFFSET,
            draw_routine_runtime_address: MDSC_MASKED_TILE_DRAW_RUNTIME_ADDRESS,
            descriptor_format: "little-endian width and height bytes followed by row-major tile indexes; 0xff leaves the destination cell unchanged",
            asset_names: &["C_CHAR1.DAT", "SELECT.DAT"],
            usage_specs: &MDSC_TEXT_USAGE_SPECS,
            transparent_tile_index: Some(0xff),
        },
    )
}

pub(super) fn analyze_opening_graphic_text_usage(
    opening_program: &GameFile,
) -> Result<GraphicTextUsageAnalysis> {
    analyze_program_graphic_text_usage(
        opening_program,
        &GraphicTextProgramSpec {
            program_name: "OPENING.COM",
            draw_routine_file_offset: OPENING_DIRECT_TILE_DRAW_FILE_OFFSET,
            draw_routine_runtime_address: OPENING_DIRECT_TILE_DRAW_RUNTIME_ADDRESS,
            descriptor_format: "little-endian width and height bytes followed by row-major tile indexes; every cell draws one tile",
            asset_names: &["CFG.DAT", "CFG_N.DAT", "TITLE.DAT"],
            usage_specs: &OPENING_TEXT_USAGE_SPECS,
            transparent_tile_index: None,
        },
    )
}

pub(super) fn analyze_ending_graphic_text_usage(
    ending_program: &GameFile,
) -> Result<GraphicTextUsageAnalysis> {
    analyze_program_graphic_text_usage(
        ending_program,
        &GraphicTextProgramSpec {
            program_name: "ENDING.COM",
            draw_routine_file_offset: ENDING_DIRECT_TILE_DRAW_FILE_OFFSET,
            draw_routine_runtime_address: ENDING_DIRECT_TILE_DRAW_RUNTIME_ADDRESS,
            descriptor_format: "little-endian width and height bytes followed by row-major tile indexes; every cell draws one tile",
            asset_names: &["FIN.DAT"],
            usage_specs: &ENDING_TEXT_USAGE_SPECS,
            transparent_tile_index: None,
        },
    )
}

fn analyze_program_graphic_text_usage(
    program: &GameFile,
    program_spec: &GraphicTextProgramSpec,
) -> Result<GraphicTextUsageAnalysis> {
    ensure!(
        program.display_name == program_spec.program_name,
        "graphic text usage requires {}",
        program_spec.program_name
    );
    let mut assets = Vec::new();
    for asset_name in program_spec.asset_names {
        let mut descriptors = Vec::new();
        let mut asset_tile_indexes = BTreeSet::new();
        let mut asset_segment_runtime_address = None;
        for spec in program_spec
            .usage_specs
            .iter()
            .filter(|spec| spec.asset_name == *asset_name)
        {
            ensure!(
                asset_segment_runtime_address
                    .is_none_or(|address| address == spec.asset_segment_runtime_address),
                "{} uses inconsistent segment slots for {asset_name}",
                program_spec.program_name
            );
            asset_segment_runtime_address = Some(spec.asset_segment_runtime_address);
            let call_site_file_offset = spec
                .call_site_runtime_address
                .checked_sub(COM_LOAD_ORIGIN)
                .context("MDSC.COM call site is below the COM load origin")?;
            let observed_call_site = program
                .bytes
                .get(
                    call_site_file_offset
                        ..call_site_file_offset + spec.expected_call_site_bytes.len(),
                )
                .with_context(|| {
                    format!(
                        "{} graphic text call site at {:#x} is truncated",
                        program_spec.program_name, spec.call_site_runtime_address
                    )
                })?;
            ensure!(
                observed_call_site == spec.expected_call_site_bytes,
                "{} graphic text call site at {:#x} changed",
                program_spec.program_name,
                spec.call_site_runtime_address
            );
            let descriptor_file_offset = spec
                .descriptor_runtime_address
                .checked_sub(COM_LOAD_ORIGIN)
                .context("MDSC.COM tile-map descriptor is below the COM load origin")?;
            let descriptor = parse_tile_map_descriptor(
                &program.bytes,
                descriptor_file_offset,
                program_spec.transparent_tile_index,
            )?;
            asset_tile_indexes.extend(&descriptor.tile_indexes);
            descriptors.push(TileMapDescriptorUsage {
                observed_role: spec.observed_role,
                call_site_file_offset,
                call_site_runtime_address: spec.call_site_runtime_address,
                descriptor_file_offset,
                descriptor_runtime_address: spec.descriptor_runtime_address,
                width_tiles: descriptor.width,
                height_tiles: descriptor.height,
                cell_count: descriptor.width * descriptor.height,
                referenced_cell_count: descriptor.referenced_cell_count,
                transparent_cell_count: descriptor.transparent_cell_count,
                unique_tile_count: descriptor.tile_indexes.len(),
                tile_index_ranges_hex: hex_tile_index_ranges(&descriptor.tile_indexes),
            });
        }
        ensure!(
            !descriptors.is_empty(),
            "no MDSC.COM graphic text descriptors found for {asset_name}"
        );
        assets.push(GraphicTextAssetUsage {
            name: asset_name,
            asset_segment_runtime_address: asset_segment_runtime_address
                .context("graphic text asset has no segment slot")?,
            descriptor_count: descriptors.len(),
            unique_tile_count: asset_tile_indexes.len(),
            tile_index_ranges_hex: hex_tile_index_ranges(&asset_tile_indexes),
            descriptors,
        });
    }

    Ok(GraphicTextUsageAnalysis {
        consumer_program: program_spec.program_name,
        draw_routine_file_offset: program_spec.draw_routine_file_offset,
        draw_routine_runtime_address: program_spec.draw_routine_runtime_address,
        descriptor_format: program_spec.descriptor_format,
        asset_count: assets.len(),
        assets,
    })
}

fn parse_tile_map_descriptor(
    bytes: &[u8],
    file_offset: usize,
    transparent_tile_index: Option<u8>,
) -> Result<ParsedTileMapDescriptor> {
    let dimensions = bytes.get(file_offset..file_offset + 2).with_context(|| {
        format!("tile-map descriptor at file offset {file_offset:#x} is truncated")
    })?;
    let width = dimensions[0] as usize;
    let height = dimensions[1] as usize;
    ensure!(
        width > 0 && height > 0,
        "tile-map dimensions must be nonzero"
    );
    let cell_count = width
        .checked_mul(height)
        .context("tile-map dimensions overflow")?;
    let cells = bytes
        .get(file_offset + 2..file_offset + 2 + cell_count)
        .with_context(|| format!("tile-map cells at file offset {file_offset:#x} are truncated"))?;
    let tile_indexes: BTreeSet<u8> = cells
        .iter()
        .copied()
        .filter(|tile_index| Some(*tile_index) != transparent_tile_index)
        .collect();
    let transparent_cell_count = cells
        .iter()
        .filter(|tile_index| Some(**tile_index) == transparent_tile_index)
        .count();
    Ok(ParsedTileMapDescriptor {
        width,
        height,
        referenced_cell_count: cell_count - transparent_cell_count,
        transparent_cell_count,
        tile_indexes,
    })
}

pub(super) fn hex_tile_index_ranges(tile_indexes: &BTreeSet<u8>) -> Vec<String> {
    let mut ranges = Vec::new();
    let mut indexes = tile_indexes.iter().copied();
    let Some(mut range_start) = indexes.next() else {
        return ranges;
    };
    let mut range_end = range_start;
    for tile_index in indexes {
        if range_end.checked_add(1) == Some(tile_index) {
            range_end = tile_index;
            continue;
        }
        ranges.push(format_hex_range(range_start, range_end));
        range_start = tile_index;
        range_end = tile_index;
    }
    ranges.push(format_hex_range(range_start, range_end));
    ranges
}

fn format_hex_range(start: u8, end: u8) -> String {
    if start == end {
        format!("{start:02x}")
    } else {
        format!("{start:02x}-{end:02x}")
    }
}

#[cfg(test)]
#[path = "graphic_text_usage_tests.rs"]
mod tests;
