use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use v30::{
    Assembler, CallTarget, CodeLocation, Condition, EffectiveAddress, EffectiveAddressBase,
    EffectiveAddressDisplacement, Instruction, LoopCondition, Operand, OperandSize, PortAddress,
    Register8, Register16, SegmentRegister, ShiftCount,
};

use crate::compile_lz::{decode_exact_compile_lz, encode_compile_lz};
use crate::expected_write::{FixedRangeExpectedWrite, apply_fixed_range_expected_writes};
use crate::font::{
    FontRole, GLYPH_HEIGHT, GLYPH_WIDTH, rasterize_character_for_role, rasterize_character_in_cell,
};
use crate::localization::{COM_LOAD_ORIGIN, require_file, sha256_hex};
use crate::masked_tile::{
    ATLAS_DECODED_SIZE, BYTES_PER_PLANE, BYTES_PER_TILE, TILE_HEIGHT, TILE_WIDTH, decode_pixel,
};
use crate::source_cd::GameFile;

fn graphic_translations_json() -> Result<&'static str> {
    crate::private_input::read_text("translations/graphic-text.json")
}
const GRAPHIC_TRANSLATION_SCHEMA: &str = "pc98_madou456.graphic_text_translations";
const TRANSLATED_STATUS: &str = "needs_human_review";
const PRESERVE_SOURCE_STATUS: &str = "preserve_source";
const EXPECTED_UNIT_COUNT: usize = 32;
const EXPECTED_TRANSLATED_UNIT_COUNT: usize = 21;
const EXPECTED_TRANSLATED_SURFACE_COUNT: usize = 31;
#[cfg(feature = "analysis")]
const CONFIGURATION_BACKING_DESCRIPTOR_OFFSET: usize = 0x2e7e;
#[cfg(feature = "analysis")]
const CONFIGURATION_BACKGROUND_DESCRIPTOR_OFFSET: usize = 0x34f4;
const CONFIGURATION_NORMAL_OBJECT_TABLE_OFFSET: usize = 0x4ede;
const CONFIGURATION_OBJECT_COUNT: usize = 10;
const CONFIGURATION_OBJECT_RECORD_SIZE: usize = 8;
const CONFIGURATION_INITIAL_SCREEN_CALL_FILE_OFFSET: usize = 0x0563;
const CONFIGURATION_ORIGINAL_INITIAL_SCREEN_RUNTIME_OFFSET: u16 = 0x1dc7;
const CONFIGURATION_BACKGROUND_SEGMENT_RUNTIME_OFFSET: u16 = 0x4306;
const CONFIGURATION_NORMAL_SEGMENT_RUNTIME_OFFSET: u16 = 0x4308;
const CONFIGURATION_SELECTED_SEGMENT_RUNTIME_OFFSET: u16 = 0x430a;
const CONFIGURATION_SELECTED_SEGMENT_LOAD_FILE_OFFSETS: [usize; 2] = [0x17b8, 0x17e2];
const CONFIGURATION_NORMAL_SEGMENT_LOAD_FILE_OFFSETS: [usize; 2] = [0x1811, 0x183b];
const CONFIGURATION_SELECTED_DRAW_CALLS: [(usize, [u8; 3]); 2] =
    [(0x17be, [0xe8, 0x92, 0x00]), (0x17e8, [0xe8, 0x68, 0x00])];
const CONFIGURATION_NORMAL_DRAW_CALLS: [(usize, [u8; 3]); 2] =
    [(0x1817, [0xe8, 0x39, 0x00]), (0x1841, [0xe8, 0x0f, 0x00])];
const CONFIGURATION_OBJECT_DRAW_RUNTIME_OFFSET: u16 = 0x1953;
const CONFIGURATION_BACKGROUND_RESTORE_RUNTIME_OFFSET: u16 = 0x1978;
const CONFIGURATION_NORMAL_OBJECT_TABLE_RUNTIME_OFFSET: u16 =
    (CONFIGURATION_NORMAL_OBJECT_TABLE_OFFSET + COM_LOAD_ORIGIN) as u16;
const CONFIGURATION_PAGE_STATE_RUNTIME_OFFSET: u16 = 0x561e;

#[derive(Debug, Clone, Eq, PartialEq, Serialize)]
pub(crate) struct GraphicLocalizationReport {
    pub(crate) translation_asset: &'static str,
    pub(crate) translation_asset_sha256: String,
    pub(crate) unit_count: usize,
    pub(crate) translated_unit_count: usize,
    pub(crate) preserve_source_unit_count: usize,
    pub(crate) changed_asset_count: usize,
    pub(crate) changed_program_count: usize,
    pub(crate) changed_file_count: usize,
    pub(crate) fixed_expected_write_count: usize,
    pub(crate) status: &'static str,
}

pub(crate) struct GraphicLocalizationOutput {
    pub(crate) replacements: BTreeMap<String, Vec<u8>>,
    pub(crate) changed_unit_counts: BTreeMap<String, usize>,
    pub(crate) fixed_expected_write_counts: BTreeMap<String, usize>,
    pub(crate) report: GraphicLocalizationReport,
    #[cfg(feature = "analysis")]
    pub(crate) configuration_mini_atlas: Vec<u8>,
}

#[cfg(feature = "analysis")]
pub(crate) struct GraphicSurfaceComparisonImage {
    pub(crate) id: String,
    pub(crate) unit_id: Option<&'static str>,
    pub(crate) source_asset: &'static str,
    pub(crate) consumer_program: &'static str,
    pub(crate) classification: &'static str,
    pub(crate) width: usize,
    pub(crate) height: usize,
    pub(crate) minimum_ink_height: Option<usize>,
    pub(crate) source_ink_bounds: Option<[usize; 4]>,
    pub(crate) localized_ink_bounds: Option<[usize; 4]>,
    pub(crate) source_ink_size: Option<[usize; 2]>,
    pub(crate) localized_ink_size: Option<[usize; 2]>,
    pub(crate) source_ink_palette_indices: Vec<u8>,
    pub(crate) localized_ink_palette_indices: Vec<u8>,
    pub(crate) protected_background_pixels_changed: Option<usize>,
    pub(crate) source_rgb: Vec<u8>,
    pub(crate) localized_rgb: Vec<u8>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct GraphicTranslationCatalog {
    schema: String,
    units: Vec<GraphicTranslationUnit>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct GraphicTranslationUnit {
    id: String,
    ko_lines: Vec<String>,
    status: String,
    notes: String,
}

#[derive(Clone, Copy)]
enum DescriptorFormat {
    ByteCells { transparent: Option<u8> },
    WordCells,
}

impl DescriptorFormat {
    fn header_size(self) -> usize {
        match self {
            Self::ByteCells { .. } => 2,
            Self::WordCells => 4,
        }
    }
}

#[derive(Clone, Copy)]
enum SurfaceLayout {
    CenteredText {
        scale: usize,
        font_role: FontRole,
    },
    NativeCenteredText {
        font_role: FontRole,
        font_size: u16,
        advance: usize,
        outline: bool,
    },
}

#[derive(Clone, Copy)]
enum MaskComposition {
    RenderedGlyphsOnly,
}

#[derive(Clone, Copy)]
enum SurfaceDrawMode {
    DirectColorPlanes,
    MaskCoverage,
}

#[derive(Clone, Copy)]
struct GraphicSurfaceSpec {
    id: &'static str,
    program: &'static str,
    target_asset: &'static str,
    foreground_source_asset: &'static str,
    descriptor_offset: usize,
    width: usize,
    height: usize,
    format: DescriptorFormat,
    layout: SurfaceLayout,
    mask_composition: MaskComposition,
    #[cfg_attr(not(feature = "analysis"), allow(dead_code))]
    draw_mode: SurfaceDrawMode,
    minimum_ink_height: Option<usize>,
}

#[derive(Clone, Copy)]
#[cfg(any(feature = "analysis", test))]
struct OwnerPlayerLabelSpec {
    id: &'static str,
    #[cfg_attr(not(feature = "analysis"), allow(dead_code))]
    frame_descriptor_offset: usize,
    label_tiles: [u8; 6],
}

#[cfg(any(feature = "analysis", test))]
const OWNER_PLAYER_LABELS: [OwnerPlayerLabelSpec; 3] = [
    OwnerPlayerLabelSpec {
        id: "selection-owner-player-1-label",
        frame_descriptor_offset: 0x24ec,
        label_tiles: [0x56, 0x57, 0x58, 0x59, 0x5a, 0x5b],
    },
    OwnerPlayerLabelSpec {
        id: "selection-owner-player-2-label",
        frame_descriptor_offset: 0x2566,
        label_tiles: [0x56, 0x57, 0x58, 0x59, 0x5a, 0x5c],
    },
    OwnerPlayerLabelSpec {
        id: "selection-owner-player-3-label",
        frame_descriptor_offset: 0x25e0,
        label_tiles: [0x56, 0x57, 0x58, 0x59, 0x5a, 0x5d],
    },
];

#[cfg(any(feature = "analysis", test))]
const TITLE_MENU_SURFACES: [GraphicSurfaceSpec; 4] = [
    direct_byte_surface(
        "title-game-start-normal",
        "OPENING.COM",
        "TITLE.DAT",
        0x3a44,
        12,
        2,
        1,
    ),
    direct_byte_surface(
        "title-config-normal",
        "OPENING.COM",
        "TITLE.DAT",
        0x3a5e,
        8,
        2,
        1,
    ),
    direct_byte_surface(
        "title-game-start-selected",
        "OPENING.COM",
        "TITLE.DAT",
        0x3a70,
        12,
        2,
        1,
    ),
    direct_byte_surface(
        "title-config-selected",
        "OPENING.COM",
        "TITLE.DAT",
        0x3a8a,
        8,
        2,
        1,
    ),
];

const TRANSLATED_SURFACES: [GraphicSurfaceSpec; EXPECTED_TRANSLATED_SURFACE_COUNT] = [
    configuration_surface(
        "config-player-selection",
        "CFG.DAT",
        0x3268,
        18,
        2,
        39,
        40,
        32,
    ),
    configuration_surface("config-quest-setting", "CFG.DAT", 0x328e, 18, 3, 39, 40, 32),
    configuration_surface("config-return", "CFG.DAT", 0x32e4, 6, 3, 42, 40, 34),
    configuration_surface("config-one-player", "CFG.DAT", 0x32f8, 12, 3, 42, 40, 34),
    configuration_surface("config-two-players", "CFG.DAT", 0x331e, 12, 3, 42, 40, 34),
    configuration_surface("config-all-players", "CFG.DAT", 0x3344, 12, 3, 42, 40, 34),
    configuration_surface("config-quest-count-3", "CFG.DAT", 0x3494, 2, 3, 46, 32, 31),
    configuration_surface("config-quest-count-5", "CFG.DAT", 0x349c, 2, 3, 46, 32, 31),
    configuration_surface("config-quest-count-7", "CFG.DAT", 0x34a4, 2, 3, 46, 32, 31),
    configuration_surface(
        "config-quest-count-all",
        "CFG.DAT",
        0x34ac,
        6,
        3,
        40,
        40,
        32,
    ),
    configuration_surface(
        "config-player-selection",
        "CFG_N.DAT",
        0x337e,
        18,
        2,
        39,
        40,
        30,
    ),
    configuration_surface(
        "config-quest-setting",
        "CFG_N.DAT",
        0x3416,
        18,
        3,
        39,
        40,
        30,
    ),
    configuration_surface("config-return", "CFG_N.DAT", 0x346c, 6, 3, 42, 40, 32),
    configuration_surface("config-one-player", "CFG_N.DAT", 0x33a4, 12, 3, 42, 40, 32),
    configuration_surface("config-two-players", "CFG_N.DAT", 0x33ca, 12, 3, 42, 40, 32),
    configuration_surface("config-all-players", "CFG_N.DAT", 0x33f0, 12, 3, 42, 40, 32),
    configuration_surface(
        "config-quest-count-3",
        "CFG_N.DAT",
        0x34c0,
        2,
        3,
        46,
        32,
        29,
    ),
    configuration_surface(
        "config-quest-count-5",
        "CFG_N.DAT",
        0x34c8,
        2,
        3,
        46,
        32,
        29,
    ),
    configuration_surface(
        "config-quest-count-7",
        "CFG_N.DAT",
        0x34d0,
        2,
        3,
        46,
        32,
        29,
    ),
    configuration_surface(
        "config-quest-count-all",
        "CFG_N.DAT",
        0x34d8,
        6,
        3,
        40,
        40,
        30,
    ),
    masked_byte_surface(
        "selection-team-prompt",
        "MDSC.COM",
        "SELECT.DAT",
        0x29d4,
        32,
        3,
        55,
        61,
        48,
    ),
    masked_byte_surface(
        "selection-owner-prompt",
        "MDSC.COM",
        "C_CHAR1.DAT",
        0x2a36,
        28,
        3,
        45,
        50,
        40,
    ),
    direct_byte_surface("ending-finished", "ENDING.COM", "FIN.DAT", 0x4add, 14, 5, 3),
    direct_byte_surface(
        "ending-play-again",
        "ENDING.COM",
        "FIN.DAT",
        0x4b47,
        23,
        6,
        3,
    ),
    word_surface("map-main", 0xc079, 6, 15),
    word_surface("map-floor-1", 0xc089, 2, 14),
    word_surface("map-floor-2", 0xc091, 2, 14),
    word_surface("map-floor-3", 0xc099, 2, 14),
    word_surface("map-castle", 0xc0a1, 2, 16),
    word_surface("map-cave", 0xc0a9, 3, 16),
    word_surface("map-basement", 0xc0b3, 3, 16),
];

const PRESERVED_UNIT_IDS: [&str; 11] = [
    "title-game-start-normal",
    "title-config-normal",
    "title-game-start-selected",
    "title-config-selected",
    "config-heading",
    "selection-player-frame",
    "selection-owner-player-1-label",
    "selection-owner-player-2-label",
    "selection-owner-player-3-label",
    "ending-copyright-credit",
    "ending-music-credit",
];

const PRESERVED_OWNER_PLAYER_LABEL_TILES: [u8; 8] =
    [0x56, 0x57, 0x58, 0x59, 0x5a, 0x5b, 0x5c, 0x5d];

const fn direct_byte_surface(
    id: &'static str,
    program: &'static str,
    asset: &'static str,
    descriptor_offset: usize,
    width: usize,
    height: usize,
    scale: usize,
) -> GraphicSurfaceSpec {
    GraphicSurfaceSpec {
        id,
        program,
        target_asset: asset,
        foreground_source_asset: asset,
        descriptor_offset,
        width,
        height,
        format: DescriptorFormat::ByteCells {
            transparent: Some(0xff),
        },
        layout: SurfaceLayout::CenteredText {
            scale,
            font_role: FontRole::Body,
        },
        mask_composition: MaskComposition::RenderedGlyphsOnly,
        draw_mode: SurfaceDrawMode::DirectColorPlanes,
        minimum_ink_height: None,
    }
}

#[allow(clippy::too_many_arguments)]
const fn masked_byte_surface(
    id: &'static str,
    program: &'static str,
    asset: &'static str,
    descriptor_offset: usize,
    width: usize,
    height: usize,
    font_size: u16,
    advance: usize,
    minimum_ink_height: usize,
) -> GraphicSurfaceSpec {
    GraphicSurfaceSpec {
        id,
        program,
        target_asset: asset,
        foreground_source_asset: asset,
        descriptor_offset,
        width,
        height,
        format: DescriptorFormat::ByteCells {
            transparent: Some(0xff),
        },
        layout: SurfaceLayout::NativeCenteredText {
            font_role: FontRole::SelectionPrompt,
            font_size,
            advance,
            outline: true,
        },
        mask_composition: MaskComposition::RenderedGlyphsOnly,
        draw_mode: SurfaceDrawMode::MaskCoverage,
        minimum_ink_height: Some(minimum_ink_height),
    }
}

#[allow(clippy::too_many_arguments)]
const fn configuration_surface(
    id: &'static str,
    asset: &'static str,
    descriptor_offset: usize,
    width: usize,
    height: usize,
    font_size: u16,
    advance: usize,
    minimum_ink_height: usize,
) -> GraphicSurfaceSpec {
    GraphicSurfaceSpec {
        id,
        program: "OPENING.COM",
        target_asset: asset,
        foreground_source_asset: asset,
        descriptor_offset,
        width,
        height,
        format: DescriptorFormat::ByteCells { transparent: None },
        layout: SurfaceLayout::NativeCenteredText {
            font_role: FontRole::UtilityLettering,
            font_size,
            advance,
            outline: true,
        },
        mask_composition: MaskComposition::RenderedGlyphsOnly,
        draw_mode: SurfaceDrawMode::MaskCoverage,
        minimum_ink_height: Some(minimum_ink_height),
    }
}

const fn word_surface(
    id: &'static str,
    descriptor_offset: usize,
    width: usize,
    minimum_ink_height: usize,
) -> GraphicSurfaceSpec {
    GraphicSurfaceSpec {
        id,
        program: "MADO456.COM",
        target_asset: "MAP_CHR.DAT",
        foreground_source_asset: "MAP_CHR.DAT",
        descriptor_offset,
        width,
        height: 1,
        format: DescriptorFormat::WordCells,
        layout: SurfaceLayout::NativeCenteredText {
            font_role: FontRole::UtilityLettering,
            font_size: 17,
            advance: 16,
            outline: true,
        },
        mask_composition: MaskComposition::RenderedGlyphsOnly,
        draw_mode: SurfaceDrawMode::DirectColorPlanes,
        minimum_ink_height: Some(minimum_ink_height),
    }
}

fn decode_graphic_atlas(source_files: &[GameFile], asset_name: &str) -> Result<Vec<u8>> {
    let packed_source = &require_file(source_files, asset_name)?.bytes;
    let decoded = decode_exact_compile_lz(packed_source)
        .with_context(|| format!("{asset_name} is not exact Compile-LZ"))?;
    ensure!(
        decoded.streams.len() == 1 && decoded.streams[0].len() == ATLAS_DECODED_SIZE,
        "{asset_name} must decode to one {ATLAS_DECODED_SIZE}-byte tile atlas"
    );
    Ok(decoded.streams[0].clone())
}

pub(crate) fn build_graphic_localization(
    source_files: &[GameFile],
    localized_programs: &BTreeMap<String, Vec<u8>>,
) -> Result<GraphicLocalizationOutput> {
    let catalog = load_graphic_translation_catalog()?;
    let units = catalog
        .units
        .iter()
        .map(|unit| (unit.id.as_str(), unit))
        .collect::<BTreeMap<_, _>>();

    let mut replacements = BTreeMap::new();
    let mut changed_unit_counts = BTreeMap::new();
    let mut fixed_expected_write_counts = BTreeMap::new();
    let mut program_writes = BTreeMap::<&str, Vec<FixedRangeExpectedWrite>>::new();
    let mut translated_by_asset =
        BTreeMap::<&str, Vec<(&GraphicSurfaceSpec, &GraphicTranslationUnit)>>::new();
    for spec in &TRANSLATED_SURFACES {
        let unit = units
            .get(spec.id)
            .with_context(|| format!("graphic translation unit {} is missing", spec.id))?;
        translated_by_asset
            .entry(spec.target_asset)
            .or_default()
            .push((spec, *unit));
    }

    let selected_configuration_surfaces = translated_by_asset
        .remove("CFG.DAT")
        .context("selected configuration surfaces are missing")?;
    let normal_configuration_surfaces = translated_by_asset
        .remove("CFG_N.DAT")
        .context("normal configuration surfaces are missing")?;
    let mut configuration_source_ink_tops = BTreeMap::<&str, usize>::new();
    for (spec, _) in selected_configuration_surfaces
        .iter()
        .chain(normal_configuration_surfaces.iter())
    {
        let source_program = &require_file(source_files, spec.program)?.bytes;
        let descriptor = parse_descriptor(source_program, spec)?;
        let source_atlas = decode_graphic_atlas(source_files, spec.target_asset)?;
        let source_ink_top = descriptor_ink_bounds(&source_atlas, &descriptor, spec)?
            .map(|bounds| bounds[1])
            .with_context(|| format!("{} source has no measurable ink", spec.id))?;
        configuration_source_ink_tops
            .entry(spec.id)
            .and_modify(|top| *top = (*top).min(source_ink_top))
            .or_insert(source_ink_top);
    }
    let mut configuration_allocator = TileAllocator::new((1..=u8::MAX).collect());
    let mut configuration_cells = BTreeMap::<&str, (TextCanvas, Vec<u8>)>::new();
    let mut configuration_readbacks = Vec::new();
    for (spec, unit) in selected_configuration_surfaces {
        let source_program = &require_file(source_files, spec.program)?.bytes;
        let descriptor = parse_descriptor(source_program, spec)?;
        let source_ink_top = configuration_source_ink_tops.get(spec.id).copied();
        let canvas = render_surface(spec, unit, 8, Some(0), source_ink_top)?;
        let replacement_cells = encode_canvas_cells(&canvas, spec, &mut configuration_allocator)
            .with_context(|| format!("could not allocate mini-atlas tiles for {}", spec.id))?;
        program_writes
            .entry(spec.program)
            .or_default()
            .push(FixedRangeExpectedWrite {
                owner: spec.id,
                purpose: "bind source-scale selected configuration lettering to the runtime mini atlas",
                offset: spec.descriptor_offset + spec.format.header_size(),
                expected_source: descriptor.source_cells,
                replacement: replacement_cells.clone(),
            });
        ensure!(
            configuration_cells
                .insert(spec.id, (canvas.clone(), replacement_cells.clone()))
                .is_none(),
            "configuration mini atlas repeats {}",
            spec.id
        );
        configuration_readbacks.push((*spec, canvas, replacement_cells));
    }
    for (spec, unit) in normal_configuration_surfaces {
        let source_program = &require_file(source_files, spec.program)?.bytes;
        let descriptor = parse_descriptor(source_program, spec)?;
        let (selected_canvas, replacement_cells) = configuration_cells
            .get(spec.id)
            .with_context(|| format!("selected configuration surface {} is missing", spec.id))?;
        let source_ink_top = configuration_source_ink_tops.get(spec.id).copied();
        let canvas = render_surface(spec, unit, 8, Some(0), source_ink_top)?;
        ensure!(
            &canvas == selected_canvas,
            "{} normal and selected configuration shapes differ",
            spec.id
        );
        program_writes
            .entry(spec.program)
            .or_default()
            .push(FixedRangeExpectedWrite {
                owner: spec.id,
                purpose: "bind the same source-scale shape to the normal configuration state",
                offset: spec.descriptor_offset + spec.format.header_size(),
                expected_source: descriptor.source_cells,
                replacement: replacement_cells.clone(),
            });
        configuration_readbacks.push((*spec, canvas, replacement_cells.clone()));
    }
    let mut configuration_mini_atlas = vec![0; ATLAS_DECODED_SIZE];
    configuration_allocator.write_tiles(&mut configuration_mini_atlas)?;
    for (spec, canvas, replacement_cells) in &configuration_readbacks {
        verify_canvas_readback(&configuration_mini_atlas, canvas, spec, replacement_cells)?;
    }
    let configuration_mini_atlas_used_size = configuration_allocator.compact_size();

    for (asset_name, surfaces) in translated_by_asset {
        let changed_unit_count = surfaces.len();
        let mut atlas = decode_graphic_atlas(source_files, asset_name)?;
        let tile_pool = verified_tile_pool(source_files, asset_name, &surfaces)?;
        let mut allocator = TileAllocator::new(tile_pool);
        let mut canvas_readbacks = Vec::new();

        for (spec, unit) in surfaces {
            let source_program = &require_file(source_files, spec.program)?.bytes;
            let descriptor = parse_descriptor(source_program, spec)?;
            let foreground_source =
                decode_graphic_atlas(source_files, spec.foreground_source_asset)?;
            let source_colors = source_text_colors(&foreground_source, &descriptor.tile_indexes)
                .with_context(|| format!("could not select source colors for {}", spec.id))?;
            let outline = source_colors
                .get(1)
                .copied()
                .or_else(|| surface_uses_outline(spec).then_some(0));
            let source_ink_top = descriptor_ink_bounds(&foreground_source, &descriptor, spec)?
                .map(|bounds| bounds[1]);
            let canvas = render_surface(spec, unit, source_colors[0], outline, source_ink_top)?;
            let replacement_cells = encode_canvas_cells(&canvas, spec, &mut allocator)
                .with_context(|| format!("could not allocate tiles for {}", spec.id))?;
            program_writes
                .entry(spec.program)
                .or_default()
                .push(FixedRangeExpectedWrite {
                    owner: spec.id,
                    purpose: "bind the translated graphic canvas to owned tile indexes",
                    offset: spec.descriptor_offset + spec.format.header_size(),
                    expected_source: descriptor.source_cells,
                    replacement: replacement_cells.clone(),
                });
            canvas_readbacks.push((*spec, canvas, replacement_cells));
        }

        allocator.write_tiles(&mut atlas)?;
        if asset_name == "C_CHAR1.DAT" {
            let source_atlas = decode_graphic_atlas(source_files, asset_name)?;
            for tile in PRESERVED_OWNER_PLAYER_LABEL_TILES {
                let start = usize::from(tile) * BYTES_PER_TILE;
                ensure!(
                    atlas.get(start..start + BYTES_PER_TILE)
                        == source_atlas.get(start..start + BYTES_PER_TILE),
                    "preserved owner-player label tile {tile:#04x} changed"
                );
            }
        }
        for (spec, canvas, replacement_cells) in &canvas_readbacks {
            verify_canvas_readback(&atlas, canvas, spec, replacement_cells)?;
        }
        let packed = encode_compile_lz(&atlas);
        let readback = decode_exact_compile_lz(&packed)
            .with_context(|| format!("rebuilt {asset_name} is not exact Compile-LZ"))?;
        ensure!(
            readback.streams.len() == 1 && readback.streams[0] == atlas,
            "rebuilt {asset_name} differs after Compile-LZ readback"
        );
        ensure!(
            replacements.insert(asset_name.to_owned(), packed).is_none(),
            "graphic localization repeats {asset_name}"
        );
        changed_unit_counts.insert(asset_name.to_owned(), changed_unit_count);
    }

    let mut fixed_expected_write_count = 0usize;
    let changed_programs = program_writes.keys().copied().collect::<BTreeSet<_>>();
    for program_name in changed_programs {
        let writes = program_writes.remove(program_name).unwrap_or_default();
        let source_program = &require_file(source_files, program_name)?.bytes;
        let base = localized_programs
            .get(program_name)
            .map(Vec::as_slice)
            .unwrap_or(source_program);
        let mut updated = apply_fixed_range_expected_writes(base, &writes)?;
        for write in &writes {
            let end = write.offset + write.replacement.len();
            ensure!(
                updated.get(write.offset..end) == Some(write.replacement.as_slice()),
                "graphic descriptor {} differs after program readback",
                write.owner
            );
        }
        let configuration_runtime_write_count = if program_name == "OPENING.COM" {
            let installed = install_configuration_mini_atlas(
                &updated,
                &configuration_mini_atlas[..configuration_mini_atlas_used_size],
            )?;
            updated = installed.program;
            installed.fixed_expected_write_count
        } else {
            0
        };
        let changed_unit_count = writes.len() + configuration_runtime_write_count;
        let expected_write_count = writes.len() + configuration_runtime_write_count;
        fixed_expected_write_count += expected_write_count;
        changed_unit_counts.insert(program_name.to_owned(), changed_unit_count);
        fixed_expected_write_counts.insert(program_name.to_owned(), expected_write_count);
        ensure!(
            replacements
                .insert(program_name.to_owned(), updated)
                .is_none(),
            "graphic localization repeats {program_name}"
        );
    }

    let changed_asset_count = replacements
        .keys()
        .filter(|name| name.ends_with(".DAT"))
        .count();
    let changed_program_count = replacements
        .keys()
        .filter(|name| name.ends_with(".COM"))
        .count();
    ensure!(
        changed_asset_count == 4 && changed_program_count == 4,
        "graphic localization changed an unexpected file population"
    );
    Ok(GraphicLocalizationOutput {
        report: GraphicLocalizationReport {
            translation_asset: "assets/translations/graphic-text.json",
            translation_asset_sha256: sha256_hex(graphic_translations_json()?.as_bytes()),
            unit_count: catalog.units.len(),
            translated_unit_count: EXPECTED_TRANSLATED_UNIT_COUNT,
            preserve_source_unit_count: PRESERVED_UNIT_IDS.len(),
            changed_asset_count,
            changed_program_count,
            changed_file_count: replacements.len(),
            fixed_expected_write_count,
            status: "all 32 program-linked graphic-text units are source-bound and owned; 21 translated units are rebuilt, source-scale configuration lettering is drawn from an appended runtime mini atlas, and the four English title-menu states, English configuration heading, PLAYER 1/2/3/4 labels and frames, and two company/music credit units preserve source by policy",
        },
        replacements,
        changed_unit_counts,
        fixed_expected_write_counts,
        #[cfg(feature = "analysis")]
        configuration_mini_atlas,
    })
}

#[cfg(feature = "analysis")]
pub(crate) fn build_graphic_surface_comparison_images(
    source_files: &[GameFile],
) -> Result<Vec<GraphicSurfaceComparisonImage>> {
    let localized = build_graphic_localization(source_files, &BTreeMap::new())?;
    let mut images = Vec::new();

    for spec in TRANSLATED_SURFACES {
        let configuration_atlas = match spec.target_asset {
            "CFG.DAT" => Some(recolor_configuration_mini_atlas(
                &localized.configuration_mini_atlas,
                true,
            )?),
            "CFG_N.DAT" => Some(recolor_configuration_mini_atlas(
                &localized.configuration_mini_atlas,
                false,
            )?),
            _ => None,
        };
        images.push(build_descriptor_comparison(
            source_files,
            &localized.replacements,
            configuration_atlas.as_deref(),
            spec,
            Some(spec.id),
            "translated",
        )?);
    }

    for label in OWNER_PLAYER_LABELS {
        let source_program = &require_file(source_files, "MDSC.COM")?.bytes;
        require_owner_player_label_descriptor(source_program, label)?;
        let source_atlas = decode_graphic_atlas(source_files, "C_CHAR1.DAT")?;
        let localized_atlas =
            decode_replacement_graphic_atlas(&localized.replacements, "C_CHAR1.DAT")?;
        let source_ink_bounds = tile_strip_ink_bounds(&source_atlas, &label.label_tiles)?;
        let localized_ink_bounds = tile_strip_ink_bounds(&localized_atlas, &label.label_tiles)?;
        images.push(GraphicSurfaceComparisonImage {
            id: label.id.to_owned(),
            unit_id: Some(label.id),
            source_asset: "C_CHAR1.DAT",
            consumer_program: "MDSC.COM",
            classification: "preserve_source",
            width: label.label_tiles.len() * TILE_WIDTH,
            height: TILE_HEIGHT,
            minimum_ink_height: None,
            source_ink_bounds,
            localized_ink_bounds,
            source_ink_size: source_ink_bounds.map(ink_bounds_size),
            localized_ink_size: localized_ink_bounds.map(ink_bounds_size),
            source_ink_palette_indices: tile_strip_ink_palette_indices(
                &source_atlas,
                &label.label_tiles,
            )?,
            localized_ink_palette_indices: tile_strip_ink_palette_indices(
                &localized_atlas,
                &label.label_tiles,
            )?,
            protected_background_pixels_changed: Some(0),
            source_rgb: render_tile_strip_rgb(&source_atlas, &label.label_tiles)?,
            localized_rgb: render_tile_strip_rgb(&localized_atlas, &label.label_tiles)?,
        });
    }

    for spec in TITLE_MENU_SURFACES.into_iter().chain([
        GraphicSurfaceSpec {
            id: "selection-player-frame",
            program: "MDSC.COM",
            target_asset: "SELECT.DAT",
            foreground_source_asset: "SELECT.DAT",
            descriptor_offset: 0x240e,
            width: 8,
            height: 9,
            format: DescriptorFormat::ByteCells {
                transparent: Some(0xff),
            },
            layout: SurfaceLayout::CenteredText {
                scale: 1,
                font_role: FontRole::Body,
            },
            mask_composition: MaskComposition::RenderedGlyphsOnly,
            draw_mode: SurfaceDrawMode::MaskCoverage,
            minimum_ink_height: None,
        },
        direct_byte_surface(
            "ending-copyright-credit",
            "ENDING.COM",
            "FIN.DAT",
            0x4b25,
            14,
            1,
            1,
        ),
        direct_byte_surface(
            "ending-music-credit",
            "ENDING.COM",
            "FIN.DAT",
            0x4b35,
            16,
            1,
            1,
        ),
    ]) {
        images.push(build_descriptor_comparison(
            source_files,
            &localized.replacements,
            None,
            spec,
            Some(spec.id),
            "preserve_source",
        )?);
    }

    images.push(build_configuration_screen_comparison(
        source_files,
        &localized,
    )?);

    ensure!(
        images.len() == 42,
        "graphic comparison surface population changed"
    );
    Ok(images)
}

#[cfg(feature = "analysis")]
fn build_descriptor_comparison(
    source_files: &[GameFile],
    localized_replacements: &BTreeMap<String, Vec<u8>>,
    localized_atlas_override: Option<&[u8]>,
    spec: GraphicSurfaceSpec,
    unit_id: Option<&'static str>,
    classification: &'static str,
) -> Result<GraphicSurfaceComparisonImage> {
    let source_program = &require_file(source_files, spec.program)?.bytes;
    let localized_program = localized_replacements
        .get(spec.program)
        .map(Vec::as_slice)
        .unwrap_or(source_program);
    let source_descriptor = parse_descriptor(source_program, &spec)?;
    let localized_descriptor = parse_descriptor(localized_program, &spec)?;
    let source_atlas = decode_graphic_atlas(source_files, spec.target_asset)?;
    let localized_atlas = if let Some(atlas) = localized_atlas_override {
        atlas.to_vec()
    } else if localized_replacements.contains_key(spec.target_asset) {
        decode_replacement_graphic_atlas(localized_replacements, spec.target_asset)?
    } else {
        ensure!(
            classification == "preserve_source",
            "translated graphic replacement {} is missing",
            spec.target_asset
        );
        source_atlas.clone()
    };
    let variant = spec
        .target_asset
        .strip_suffix(".DAT")
        .unwrap_or(spec.target_asset)
        .to_ascii_lowercase();
    let id = if matches!(spec.target_asset, "CFG.DAT" | "CFG_N.DAT") && unit_id.is_some() {
        format!("{}--{variant}", spec.id)
    } else {
        spec.id.to_owned()
    };
    let source_ink_bounds = descriptor_ink_bounds(&source_atlas, &source_descriptor, &spec)?;
    let localized_ink_bounds =
        descriptor_ink_bounds(&localized_atlas, &localized_descriptor, &spec)?;
    let source_ink_size = source_ink_bounds.map(ink_bounds_size);
    let localized_ink_size = localized_ink_bounds.map(ink_bounds_size);
    let source_ink_palette_indices =
        descriptor_ink_palette_indices(&source_atlas, &source_descriptor, &spec)?;
    let localized_ink_palette_indices =
        descriptor_ink_palette_indices(&localized_atlas, &localized_descriptor, &spec)?;
    let source_rgb = render_descriptor_rgb(&source_atlas, &source_descriptor, &spec)?;
    let localized_rgb = render_descriptor_rgb(&localized_atlas, &localized_descriptor, &spec)?;
    let protected_background_pixels_changed =
        if matches!(spec.draw_mode, SurfaceDrawMode::MaskCoverage) {
            let source_mask = render_descriptor_mask(&source_atlas, &source_descriptor, &spec)?;
            let localized_mask =
                render_descriptor_mask(&localized_atlas, &localized_descriptor, &spec)?;
            Some(count_changed_rgb_pixels_outside_masks(
                &source_rgb,
                &localized_rgb,
                &source_mask,
                &localized_mask,
            )?)
        } else {
            None
        };
    Ok(GraphicSurfaceComparisonImage {
        id,
        unit_id,
        source_asset: spec.target_asset,
        consumer_program: spec.program,
        classification,
        width: spec.width * TILE_WIDTH,
        height: spec.height * TILE_HEIGHT,
        minimum_ink_height: spec.minimum_ink_height,
        source_ink_bounds,
        localized_ink_bounds,
        source_ink_size,
        localized_ink_size,
        source_ink_palette_indices,
        localized_ink_palette_indices,
        protected_background_pixels_changed,
        source_rgb,
        localized_rgb,
    })
}

#[cfg(feature = "analysis")]
fn build_configuration_screen_comparison(
    source_files: &[GameFile],
    localized: &GraphicLocalizationOutput,
) -> Result<GraphicSurfaceComparisonImage> {
    let screen_spec = GraphicSurfaceSpec {
        id: "configuration-composed-normal-screen",
        program: "OPENING.COM",
        target_asset: "CFG_N.DAT",
        foreground_source_asset: "CFG_N.DAT",
        descriptor_offset: CONFIGURATION_BACKING_DESCRIPTOR_OFFSET,
        width: 40,
        height: 25,
        format: DescriptorFormat::ByteCells { transparent: None },
        layout: SurfaceLayout::CenteredText {
            scale: 1,
            font_role: FontRole::UtilityLettering,
        },
        mask_composition: MaskComposition::RenderedGlyphsOnly,
        draw_mode: SurfaceDrawMode::MaskCoverage,
        minimum_ink_height: None,
    };
    let source_program = &require_file(source_files, "OPENING.COM")?.bytes;
    let localized_program = localized
        .replacements
        .get("OPENING.COM")
        .context("localized OPENING.COM is missing")?;
    let backing_descriptor = parse_descriptor(source_program, &screen_spec)?;
    let background_spec = GraphicSurfaceSpec {
        id: "configuration-background-screen",
        descriptor_offset: CONFIGURATION_BACKGROUND_DESCRIPTOR_OFFSET,
        draw_mode: SurfaceDrawMode::DirectColorPlanes,
        ..screen_spec
    };
    let background_descriptor = parse_descriptor(source_program, &background_spec)?;
    let source_atlas = decode_graphic_atlas(source_files, "CFG_N.DAT")?;
    let background_atlas = decode_graphic_atlas(source_files, "BG_S.DAT")?;
    let normal_atlas =
        recolor_configuration_mini_atlas(&localized.configuration_mini_atlas, false)?;
    let mut source_rgb =
        render_descriptor_rgb(&background_atlas, &background_descriptor, &background_spec)?;
    overlay_descriptor_rgb(
        &mut source_rgb,
        screen_spec.width * TILE_WIDTH,
        0,
        0,
        &source_atlas,
        &backing_descriptor,
        &screen_spec,
    )?;
    let mut localized_rgb = source_rgb.clone();
    let screen_pixel_count = screen_spec.width * TILE_WIDTH * screen_spec.height * TILE_HEIGHT;
    let mut source_ink_mask = vec![false; screen_pixel_count];
    let mut localized_ink_mask = vec![false; screen_pixel_count];
    let normal_specs = TRANSLATED_SURFACES
        .iter()
        .filter(|spec| spec.target_asset == "CFG_N.DAT")
        .collect::<Vec<_>>();
    ensure!(
        normal_specs.len() == CONFIGURATION_OBJECT_COUNT,
        "normal configuration comparison object population changed"
    );
    for spec in normal_specs {
        let placement = configuration_normal_object_placement(source_program, spec)?;
        let object_background_descriptor = descriptor_rectangle(
            &background_descriptor,
            screen_spec.width,
            placement,
            spec.id,
        )?;
        let object_background_spec = GraphicSurfaceSpec {
            draw_mode: SurfaceDrawMode::DirectColorPlanes,
            ..*spec
        };
        overlay_descriptor_rgb(
            &mut source_rgb,
            screen_spec.width * TILE_WIDTH,
            placement.column * TILE_WIDTH,
            placement.row * TILE_HEIGHT,
            &background_atlas,
            &object_background_descriptor,
            &object_background_spec,
        )?;
        overlay_descriptor_rgb(
            &mut localized_rgb,
            screen_spec.width * TILE_WIDTH,
            placement.column * TILE_WIDTH,
            placement.row * TILE_HEIGHT,
            &background_atlas,
            &object_background_descriptor,
            &object_background_spec,
        )?;
        let source_descriptor = parse_descriptor(source_program, spec)?;
        overlay_descriptor_rgb(
            &mut source_rgb,
            screen_spec.width * TILE_WIDTH,
            placement.column * TILE_WIDTH,
            placement.row * TILE_HEIGHT,
            &source_atlas,
            &source_descriptor,
            spec,
        )?;
        mark_descriptor_mask(
            &mut source_ink_mask,
            screen_spec.width * TILE_WIDTH,
            placement.column * TILE_WIDTH,
            placement.row * TILE_HEIGHT,
            &source_atlas,
            &source_descriptor,
            spec,
        )?;
        let localized_descriptor = parse_descriptor(localized_program, spec)?;
        overlay_descriptor_rgb(
            &mut localized_rgb,
            screen_spec.width * TILE_WIDTH,
            placement.column * TILE_WIDTH,
            placement.row * TILE_HEIGHT,
            &normal_atlas,
            &localized_descriptor,
            spec,
        )?;
        mark_descriptor_mask(
            &mut localized_ink_mask,
            screen_spec.width * TILE_WIDTH,
            placement.column * TILE_WIDTH,
            placement.row * TILE_HEIGHT,
            &normal_atlas,
            &localized_descriptor,
            spec,
        )?;
    }
    let protected_background_pixels_changed = count_changed_rgb_pixels_outside_masks(
        &source_rgb,
        &localized_rgb,
        &source_ink_mask,
        &localized_ink_mask,
    )?;
    ensure!(
        protected_background_pixels_changed == 0,
        "localized configuration composition changed {protected_background_pixels_changed} protected background pixels"
    );
    Ok(GraphicSurfaceComparisonImage {
        id: screen_spec.id.to_owned(),
        unit_id: None,
        source_asset: screen_spec.target_asset,
        consumer_program: screen_spec.program,
        classification: "composed_screen",
        width: screen_spec.width * TILE_WIDTH,
        height: screen_spec.height * TILE_HEIGHT,
        minimum_ink_height: None,
        source_ink_bounds: None,
        localized_ink_bounds: None,
        source_ink_size: None,
        localized_ink_size: None,
        source_ink_palette_indices: Vec::new(),
        localized_ink_palette_indices: Vec::new(),
        protected_background_pixels_changed: Some(protected_background_pixels_changed),
        source_rgb,
        localized_rgb,
    })
}

#[cfg(feature = "analysis")]
fn descriptor_rectangle(
    descriptor: &ParsedDescriptor,
    descriptor_width: usize,
    placement: ConfigurationObjectPlacement,
    id: &str,
) -> Result<ParsedDescriptor> {
    ensure!(
        descriptor
            .source_cells
            .len()
            .is_multiple_of(descriptor_width)
            && placement.column + placement.width <= descriptor_width
            && placement.row + placement.height <= descriptor.source_cells.len() / descriptor_width,
        "{id} background rectangle is outside the configuration backing"
    );
    let mut cells = Vec::with_capacity(placement.width * placement.height);
    for row in placement.row..placement.row + placement.height {
        let start = row * descriptor_width + placement.column;
        cells.extend_from_slice(&descriptor.source_cells[start..start + placement.width]);
    }
    Ok(ParsedDescriptor {
        tile_indexes: cells.clone(),
        source_cells: cells,
    })
}

#[cfg(feature = "analysis")]
fn overlay_descriptor_rgb(
    target: &mut [u8],
    target_width: usize,
    target_x: usize,
    target_y: usize,
    atlas: &[u8],
    descriptor: &ParsedDescriptor,
    spec: &GraphicSurfaceSpec,
) -> Result<()> {
    ensure!(
        descriptor.source_cells.len() == spec.width * spec.height
            && target.len().is_multiple_of(target_width * 3)
            && target_x + spec.width * TILE_WIDTH <= target_width
            && target_y + spec.height * TILE_HEIGHT <= target.len() / (target_width * 3),
        "{} overlay has invalid RGB geometry",
        spec.id
    );
    for (cell_index, tile) in descriptor.source_cells.iter().copied().enumerate() {
        let cell_x = cell_index % spec.width;
        let cell_y = cell_index / spec.width;
        for local_y in 0..TILE_HEIGHT {
            for local_x in 0..TILE_WIDTH {
                let pixel = decode_pixel(atlas, usize::from(tile), local_x, local_y)?;
                if matches!(spec.draw_mode, SurfaceDrawMode::MaskCoverage) && !pixel.mask {
                    continue;
                }
                let x = target_x + cell_x * TILE_WIDTH + local_x;
                let y = target_y + cell_y * TILE_HEIGHT + local_y;
                let start = (y * target_width + x) * 3;
                target[start..start + 3].copy_from_slice(&comparison_rgb(pixel.color_index));
            }
        }
    }
    Ok(())
}

#[cfg(feature = "analysis")]
fn mark_descriptor_mask(
    target: &mut [bool],
    target_width: usize,
    target_x: usize,
    target_y: usize,
    atlas: &[u8],
    descriptor: &ParsedDescriptor,
    spec: &GraphicSurfaceSpec,
) -> Result<()> {
    let local_mask = render_descriptor_mask(atlas, descriptor, spec)?;
    let local_width = spec.width * TILE_WIDTH;
    let local_height = spec.height * TILE_HEIGHT;
    ensure!(
        target.len().is_multiple_of(target_width)
            && target_x + local_width <= target_width
            && target_y + local_height <= target.len() / target_width,
        "{} mask placement has invalid geometry",
        spec.id
    );
    for y in 0..local_height {
        for x in 0..local_width {
            if local_mask[y * local_width + x] {
                target[(target_y + y) * target_width + target_x + x] = true;
            }
        }
    }
    Ok(())
}

#[cfg(any(feature = "analysis", test))]
fn recolor_configuration_mini_atlas(atlas: &[u8], selected: bool) -> Result<Vec<u8>> {
    ensure!(
        atlas.len().is_multiple_of(BYTES_PER_TILE),
        "configuration mini atlas has invalid tile geometry"
    );
    let mut recolored = atlas.to_vec();
    let (records, remainder) = recolored.as_chunks_mut::<BYTES_PER_TILE>();
    debug_assert!(remainder.is_empty());
    for record in records {
        let mask = record[..BYTES_PER_PLANE].to_vec();
        let fill = record[4 * BYTES_PER_PLANE..5 * BYTES_PER_PLANE].to_vec();
        if selected {
            record[BYTES_PER_PLANE..2 * BYTES_PER_PLANE].copy_from_slice(&mask);
            record[2 * BYTES_PER_PLANE..3 * BYTES_PER_PLANE].copy_from_slice(&mask);
            record[3 * BYTES_PER_PLANE..4 * BYTES_PER_PLANE].copy_from_slice(&fill);
        } else {
            record[BYTES_PER_PLANE..2 * BYTES_PER_PLANE].fill(0);
            record[2 * BYTES_PER_PLANE..3 * BYTES_PER_PLANE].copy_from_slice(&fill);
            record[3 * BYTES_PER_PLANE..4 * BYTES_PER_PLANE].fill(0);
        }
    }
    Ok(recolored)
}

#[cfg(feature = "analysis")]
fn decode_replacement_graphic_atlas(
    replacements: &BTreeMap<String, Vec<u8>>,
    asset_name: &str,
) -> Result<Vec<u8>> {
    let packed = replacements
        .get(asset_name)
        .with_context(|| format!("localized graphic replacement {asset_name} is missing"))?;
    let decoded = decode_exact_compile_lz(packed)
        .with_context(|| format!("localized {asset_name} is not exact Compile-LZ"))?;
    ensure!(
        decoded.streams.len() == 1 && decoded.streams[0].len() == ATLAS_DECODED_SIZE,
        "localized {asset_name} must decode to one {ATLAS_DECODED_SIZE}-byte tile atlas"
    );
    Ok(decoded.streams[0].clone())
}

#[cfg(feature = "analysis")]
fn render_descriptor_rgb(
    atlas: &[u8],
    descriptor: &ParsedDescriptor,
    spec: &GraphicSurfaceSpec,
) -> Result<Vec<u8>> {
    let width = spec.width * TILE_WIDTH;
    let height = spec.height * TILE_HEIGHT;
    let cell_size = match spec.format {
        DescriptorFormat::ByteCells { .. } => 1,
        DescriptorFormat::WordCells => 2,
    };
    ensure!(
        descriptor.source_cells.len() == spec.width * spec.height * cell_size,
        "{} comparison descriptor has the wrong cell count",
        spec.id
    );
    let mut rgb = vec![0_u8; width * height * 3];
    for (cell_index, cell) in descriptor.source_cells.chunks_exact(cell_size).enumerate() {
        let transparent = matches!(
            spec.format,
            DescriptorFormat::ByteCells {
                transparent: Some(value)
            } if cell[0] == value
        );
        if transparent {
            continue;
        }
        let tile = usize::from(cell[0]);
        let cell_x = cell_index % spec.width;
        let cell_y = cell_index / spec.width;
        for local_y in 0..TILE_HEIGHT {
            for local_x in 0..TILE_WIDTH {
                let pixel = decode_pixel(atlas, tile, local_x, local_y)?;
                if matches!(spec.draw_mode, SurfaceDrawMode::MaskCoverage) && !pixel.mask {
                    continue;
                }
                let x = cell_x * TILE_WIDTH + local_x;
                let y = cell_y * TILE_HEIGHT + local_y;
                let start = (y * width + x) * 3;
                rgb[start..start + 3].copy_from_slice(&comparison_rgb(pixel.color_index));
            }
        }
    }
    Ok(rgb)
}

#[cfg(feature = "analysis")]
fn render_descriptor_mask(
    atlas: &[u8],
    descriptor: &ParsedDescriptor,
    spec: &GraphicSurfaceSpec,
) -> Result<Vec<bool>> {
    let width = spec.width * TILE_WIDTH;
    let height = spec.height * TILE_HEIGHT;
    let cell_size = match spec.format {
        DescriptorFormat::ByteCells { .. } => 1,
        DescriptorFormat::WordCells => 2,
    };
    ensure!(
        descriptor.source_cells.len() == spec.width * spec.height * cell_size,
        "{} comparison descriptor has the wrong cell count",
        spec.id
    );
    let mut mask = vec![false; width * height];
    for (cell_index, cell) in descriptor.source_cells.chunks_exact(cell_size).enumerate() {
        let transparent = matches!(
            spec.format,
            DescriptorFormat::ByteCells {
                transparent: Some(value)
            } if cell[0] == value
        );
        if transparent {
            continue;
        }
        let tile = usize::from(cell[0]);
        let cell_x = cell_index % spec.width;
        let cell_y = cell_index / spec.width;
        for local_y in 0..TILE_HEIGHT {
            for local_x in 0..TILE_WIDTH {
                if decode_pixel(atlas, tile, local_x, local_y)?.mask {
                    let x = cell_x * TILE_WIDTH + local_x;
                    let y = cell_y * TILE_HEIGHT + local_y;
                    mask[y * width + x] = true;
                }
            }
        }
    }
    Ok(mask)
}

#[cfg(feature = "analysis")]
fn count_changed_rgb_pixels_outside_masks(
    source_rgb: &[u8],
    localized_rgb: &[u8],
    source_mask: &[bool],
    localized_mask: &[bool],
) -> Result<usize> {
    ensure!(
        source_rgb.len() == localized_rgb.len()
            && source_rgb.len() == source_mask.len() * 3
            && source_mask.len() == localized_mask.len(),
        "protected-background comparison has invalid geometry"
    );
    Ok(source_rgb
        .as_chunks::<3>()
        .0
        .iter()
        .zip(localized_rgb.as_chunks::<3>().0)
        .zip(source_mask.iter().zip(localized_mask))
        .filter(|((source, localized), (source_ink, localized_ink))| {
            !**source_ink && !**localized_ink && source != localized
        })
        .count())
}

#[cfg(feature = "analysis")]
fn descriptor_ink_palette_indices(
    atlas: &[u8],
    descriptor: &ParsedDescriptor,
    spec: &GraphicSurfaceSpec,
) -> Result<Vec<u8>> {
    let cell_size = match spec.format {
        DescriptorFormat::ByteCells { .. } => 1,
        DescriptorFormat::WordCells => 2,
    };
    ensure!(
        descriptor.source_cells.len() == spec.width * spec.height * cell_size,
        "{} comparison descriptor has the wrong cell count",
        spec.id
    );
    let mut colors = BTreeSet::new();
    for cell in descriptor.source_cells.chunks_exact(cell_size) {
        let transparent = matches!(
            spec.format,
            DescriptorFormat::ByteCells {
                transparent: Some(value)
            } if cell[0] == value
        );
        if transparent {
            continue;
        }
        let tile = usize::from(cell[0]);
        for y in 0..TILE_HEIGHT {
            for x in 0..TILE_WIDTH {
                let pixel = decode_pixel(atlas, tile, x, y)?;
                if pixel.mask {
                    colors.insert(pixel.color_index);
                }
            }
        }
    }
    Ok(colors.into_iter().collect())
}

#[cfg(feature = "analysis")]
fn tile_strip_ink_palette_indices(atlas: &[u8], tiles: &[u8]) -> Result<Vec<u8>> {
    let mut colors = BTreeSet::new();
    for tile in tiles {
        for y in 0..TILE_HEIGHT {
            for x in 0..TILE_WIDTH {
                let pixel = decode_pixel(atlas, usize::from(*tile), x, y)?;
                if pixel.mask {
                    colors.insert(pixel.color_index);
                }
            }
        }
    }
    Ok(colors.into_iter().collect())
}

#[cfg(feature = "analysis")]
fn render_tile_strip_rgb(atlas: &[u8], tiles: &[u8]) -> Result<Vec<u8>> {
    let width = tiles.len() * TILE_WIDTH;
    let mut rgb = vec![0_u8; width * TILE_HEIGHT * 3];
    for (tile_x, tile) in tiles.iter().enumerate() {
        for y in 0..TILE_HEIGHT {
            for x in 0..TILE_WIDTH {
                let pixel = decode_pixel(atlas, usize::from(*tile), x, y)?;
                if !pixel.mask {
                    continue;
                }
                let start = (y * width + tile_x * TILE_WIDTH + x) * 3;
                rgb[start..start + 3].copy_from_slice(&comparison_rgb(pixel.color_index));
            }
        }
    }
    Ok(rgb)
}

fn descriptor_ink_bounds(
    atlas: &[u8],
    descriptor: &ParsedDescriptor,
    spec: &GraphicSurfaceSpec,
) -> Result<Option<[usize; 4]>> {
    let cell_size = match spec.format {
        DescriptorFormat::ByteCells { .. } => 1,
        DescriptorFormat::WordCells => 2,
    };
    let cells = descriptor.source_cells.chunks_exact(cell_size);
    ensure!(
        cells.remainder().is_empty() && cells.len() == spec.width * spec.height,
        "{} ink measurement descriptor has the wrong cell count",
        spec.id
    );
    let transparent = match spec.format {
        DescriptorFormat::ByteCells { transparent } => transparent,
        DescriptorFormat::WordCells => None,
    };
    descriptor_tiles_ink_bounds(atlas, cells.map(|cell| cell[0]), spec.width, transparent)
}

#[cfg(feature = "analysis")]
fn tile_strip_ink_bounds(atlas: &[u8], tiles: &[u8]) -> Result<Option<[usize; 4]>> {
    descriptor_tiles_ink_bounds(atlas, tiles.iter().copied(), tiles.len(), None)
}

fn descriptor_tiles_ink_bounds(
    atlas: &[u8],
    tiles: impl Iterator<Item = u8>,
    width_in_tiles: usize,
    transparent: Option<u8>,
) -> Result<Option<[usize; 4]>> {
    let mut bounds: Option<(usize, usize, usize, usize)> = None;
    for (cell_index, tile) in tiles.enumerate() {
        if transparent == Some(tile) {
            continue;
        }
        for local_y in 0..TILE_HEIGHT {
            for local_x in 0..TILE_WIDTH {
                if !decode_pixel(atlas, usize::from(tile), local_x, local_y)?.mask {
                    continue;
                }
                let x = cell_index % width_in_tiles * TILE_WIDTH + local_x;
                let y = cell_index / width_in_tiles * TILE_HEIGHT + local_y;
                bounds = Some(match bounds {
                    Some((min_x, min_y, max_x, max_y)) => {
                        (min_x.min(x), min_y.min(y), max_x.max(x), max_y.max(y))
                    }
                    None => (x, y, x, y),
                });
            }
        }
    }
    Ok(bounds
        .map(|(min_x, min_y, max_x, max_y)| [min_x, min_y, max_x - min_x + 1, max_y - min_y + 1]))
}

#[cfg(feature = "analysis")]
const fn ink_bounds_size(bounds: [usize; 4]) -> [usize; 2] {
    [bounds[2], bounds[3]]
}

#[cfg(feature = "analysis")]
fn comparison_rgb(color_index: u8) -> [u8; 3] {
    let intensity = if color_index & 0x08 != 0 { 0xff } else { 0x80 };
    let channel = |bit| {
        if color_index & bit != 0 {
            intensity
        } else if color_index & 0x08 != 0 {
            0x40
        } else {
            0x00
        }
    };
    [channel(0x02), channel(0x04), channel(0x01)]
}

fn load_graphic_translation_catalog() -> Result<GraphicTranslationCatalog> {
    let catalog: GraphicTranslationCatalog = serde_json::from_str(graphic_translations_json()?)
        .context("could not parse tracked graphic-text translations")?;
    ensure!(
        catalog.schema == GRAPHIC_TRANSLATION_SCHEMA,
        "unsupported graphic-text translation schema"
    );
    ensure!(
        catalog.units.len() == EXPECTED_UNIT_COUNT,
        "graphic-text catalog must contain exactly {EXPECTED_UNIT_COUNT} units"
    );
    let expected_ids = TRANSLATED_SURFACES
        .iter()
        .map(|spec| spec.id)
        .chain(PRESERVED_UNIT_IDS)
        .collect::<BTreeSet<_>>();
    let observed_ids = catalog
        .units
        .iter()
        .map(|unit| unit.id.as_str())
        .collect::<BTreeSet<_>>();
    ensure!(
        observed_ids.len() == catalog.units.len() && observed_ids == expected_ids,
        "graphic-text catalog ids are missing, duplicated, or unexpected"
    );
    for unit in &catalog.units {
        ensure!(
            !unit.notes.trim().is_empty(),
            "{} has no review note",
            unit.id
        );
        if PRESERVED_UNIT_IDS.contains(&unit.id.as_str()) {
            ensure!(
                unit.status == PRESERVE_SOURCE_STATUS && unit.ko_lines.is_empty(),
                "{} must preserve source without Korean lines",
                unit.id
            );
        } else {
            ensure!(
                unit.status == TRANSLATED_STATUS && !unit.ko_lines.is_empty(),
                "{} must contain a human-review translation",
                unit.id
            );
            for line in &unit.ko_lines {
                ensure!(
                    !line.is_empty()
                        && line
                            .chars()
                            .all(|character| character == ' ' || !character.is_whitespace()),
                    "{} contains empty or unsupported whitespace",
                    unit.id
                );
            }
        }
    }
    Ok(catalog)
}

struct ParsedDescriptor {
    source_cells: Vec<u8>,
    tile_indexes: Vec<u8>,
}

#[cfg(feature = "analysis")]
#[derive(Clone, Copy)]
struct ConfigurationObjectPlacement {
    column: usize,
    row: usize,
    width: usize,
    height: usize,
}

fn parse_descriptor(program: &[u8], spec: &GraphicSurfaceSpec) -> Result<ParsedDescriptor> {
    let (width, height, source_cells, tile_indexes) = match spec.format {
        DescriptorFormat::ByteCells { transparent } => {
            let dimensions = program
                .get(spec.descriptor_offset..spec.descriptor_offset + 2)
                .context("graphic byte-cell descriptor is truncated")?;
            let width = dimensions[0] as usize;
            let height = dimensions[1] as usize;
            let cell_count = width
                .checked_mul(height)
                .context("graphic byte-cell descriptor dimensions overflow")?;
            let cells = program
                .get(spec.descriptor_offset + 2..spec.descriptor_offset + 2 + cell_count)
                .context("graphic byte-cell descriptor cells are truncated")?
                .to_vec();
            let tile_indexes = cells
                .iter()
                .copied()
                .filter(|tile| transparent != Some(*tile))
                .collect();
            (width, height, cells, tile_indexes)
        }
        DescriptorFormat::WordCells => {
            let header = program
                .get(spec.descriptor_offset..spec.descriptor_offset + 4)
                .context("graphic word-cell descriptor is truncated")?;
            let width = u16::from_le_bytes([header[0], header[1]]) as usize;
            let height = u16::from_le_bytes([header[2], header[3]]) as usize;
            let byte_count = width
                .checked_mul(height)
                .and_then(|count| count.checked_mul(2))
                .context("graphic word-cell descriptor dimensions overflow")?;
            let cells = program
                .get(spec.descriptor_offset + 4..spec.descriptor_offset + 4 + byte_count)
                .context("graphic word-cell descriptor cells are truncated")?
                .to_vec();
            let tile_indexes = cells
                .as_chunks::<2>()
                .0
                .iter()
                .map(|cell| cell[0])
                .collect();
            (width, height, cells, tile_indexes)
        }
    };
    ensure!(
        width == spec.width && height == spec.height,
        "graphic descriptor {} dimensions changed from {}x{} to {width}x{height}",
        spec.id,
        spec.width,
        spec.height
    );
    Ok(ParsedDescriptor {
        source_cells,
        tile_indexes,
    })
}

#[cfg(feature = "analysis")]
fn configuration_normal_object_placement(
    program: &[u8],
    spec: &GraphicSurfaceSpec,
) -> Result<ConfigurationObjectPlacement> {
    ensure!(
        spec.target_asset == "CFG_N.DAT"
            && matches!(spec.format, DescriptorFormat::ByteCells { .. }),
        "{} is not a normal-state configuration object",
        spec.id
    );
    let table = program
        .get(
            CONFIGURATION_NORMAL_OBJECT_TABLE_OFFSET
                ..CONFIGURATION_NORMAL_OBJECT_TABLE_OFFSET
                    + CONFIGURATION_OBJECT_COUNT * CONFIGURATION_OBJECT_RECORD_SIZE,
        )
        .context("OPENING.COM normal-state configuration object table is truncated")?;
    let expected_runtime_address = spec.descriptor_offset + COM_LOAD_ORIGIN;
    let matching_records = table
        .as_chunks::<CONFIGURATION_OBJECT_RECORD_SIZE>()
        .0
        .iter()
        .filter(|record| {
            usize::from(u16::from_le_bytes([record[0], record[1]])) == expected_runtime_address
        })
        .collect::<Vec<_>>();
    ensure!(
        matching_records.len() == 1,
        "{} does not have one normal-state configuration object record",
        spec.id
    );
    let record = matching_records[0];
    let x = usize::from(u16::from_le_bytes([record[2], record[3]]));
    let y = usize::from(u16::from_le_bytes([record[4], record[5]]));
    let width = usize::from(record[6]);
    let height = usize::from(record[7]);
    ensure!(
        x.is_multiple_of(TILE_WIDTH)
            && y.is_multiple_of(TILE_HEIGHT)
            && width == spec.width
            && height == spec.height,
        "{} normal-state configuration placement changed",
        spec.id
    );
    Ok(ConfigurationObjectPlacement {
        column: x / TILE_WIDTH,
        row: y / TILE_HEIGHT,
        width,
        height,
    })
}

fn verified_tile_pool(
    source_files: &[GameFile],
    asset_name: &str,
    surfaces: &[(&GraphicSurfaceSpec, &GraphicTranslationUnit)],
) -> Result<Vec<u8>> {
    let mut source_tiles = BTreeSet::new();
    for (spec, _) in surfaces {
        let program = &require_file(source_files, spec.program)?.bytes;
        source_tiles.extend(parse_descriptor(program, spec)?.tile_indexes);
    }
    ensure!(
        !matches!(asset_name, "CFG.DAT" | "CFG_N.DAT"),
        "configuration lettering must use the appended runtime mini atlas"
    );
    let pool = match asset_name {
        "FIN.DAT" => (0x01..=0x34).chain(0x51..=0xaa).collect::<BTreeSet<_>>(),
        "MAP_CHR.DAT" => (0x17..=0x28).collect(),
        _ => source_tiles.clone(),
    };
    ensure!(
        !pool.is_empty() && pool.is_subset(&source_tiles),
        "{asset_name} translated tile pool is not source-owned by its translated descriptors"
    );
    Ok(pool.into_iter().collect())
}

fn source_text_colors(atlas: &[u8], tile_indexes: &[u8]) -> Result<Vec<u8>> {
    let mut counts = [0usize; 16];
    for tile in tile_indexes {
        for y in 0..TILE_HEIGHT {
            for x in 0..TILE_WIDTH {
                let pixel = decode_pixel(atlas, usize::from(*tile), x, y)?;
                if pixel.color_index != 0 {
                    counts[usize::from(pixel.color_index)] += 1;
                }
            }
        }
    }
    let mut colors = counts
        .iter()
        .enumerate()
        .skip(1)
        .filter(|(_, count)| **count > 0)
        .map(|(color, count)| (color as u8, *count))
        .collect::<Vec<_>>();
    colors.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));
    ensure!(
        !colors.is_empty(),
        "graphic surface has no visible source color"
    );
    Ok(colors.into_iter().map(|(color, _)| color).collect())
}

#[cfg(feature = "analysis")]
fn require_owner_player_label_descriptor(program: &[u8], spec: OwnerPlayerLabelSpec) -> Result<()> {
    let header = program
        .get(spec.frame_descriptor_offset..spec.frame_descriptor_offset + 2)
        .context("owner-player frame descriptor is truncated")?;
    ensure!(
        header == [8, 15],
        "{} frame descriptor changed from 8x15",
        spec.id
    );
    let label_start = spec.frame_descriptor_offset + 3;
    ensure!(
        program.get(label_start..label_start + spec.label_tiles.len())
            == Some(spec.label_tiles.as_slice()),
        "{} source label tiles changed",
        spec.id
    );
    Ok(())
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct TextCanvas {
    width: usize,
    height: usize,
    pixels: Vec<Option<u8>>,
}

fn render_surface(
    spec: &GraphicSurfaceSpec,
    unit: &GraphicTranslationUnit,
    foreground: u8,
    outline: Option<u8>,
    target_ink_top: Option<usize>,
) -> Result<TextCanvas> {
    let mut canvas = TextCanvas {
        width: spec.width * TILE_WIDTH,
        height: spec.height * TILE_HEIGHT,
        pixels: vec![None; spec.width * TILE_WIDTH * spec.height * TILE_HEIGHT],
    };
    match spec.layout {
        SurfaceLayout::CenteredText { scale, font_role } => {
            canvas.draw_centered_lines_with_font(
                &unit.ko_lines,
                scale,
                0,
                canvas.height,
                font_role,
                foreground,
            )?;
        }
        SurfaceLayout::NativeCenteredText {
            font_role,
            font_size,
            advance,
            outline: use_outline,
        } => {
            canvas.draw_native_centered_lines(
                &unit.ko_lines,
                font_role,
                font_size,
                advance,
                foreground,
            )?;
            if use_outline && let Some(outline) = outline.filter(|color| *color != foreground) {
                canvas.add_outline(outline);
            }
        }
    }
    if let Some(target_ink_top) = target_ink_top {
        canvas
            .align_ink_top(target_ink_top)
            .with_context(|| format!("align {} Korean ink to source top", spec.id))?;
    }
    ensure!(
        canvas.pixels.iter().any(Option::is_some),
        "{} rendered an empty graphic canvas",
        spec.id
    );
    if let Some(minimum_ink_height) = spec.minimum_ink_height {
        let (_, ink_height) = canvas
            .ink_size()
            .with_context(|| format!("{} rendered no measurable ink", spec.id))?;
        ensure!(
            ink_height >= minimum_ink_height,
            "{} Korean ink height {ink_height} is smaller than the source minimum {minimum_ink_height}",
            spec.id
        );
    }
    Ok(canvas)
}

fn surface_uses_outline(spec: &GraphicSurfaceSpec) -> bool {
    matches!(
        spec.layout,
        SurfaceLayout::NativeCenteredText { outline: true, .. }
    )
}

impl TextCanvas {
    fn ink_bounds(&self) -> Option<(usize, usize, usize, usize)> {
        let mut min_x = self.width;
        let mut min_y = self.height;
        let mut max_x = 0usize;
        let mut max_y = 0usize;
        let mut found = false;
        for (index, pixel) in self.pixels.iter().enumerate() {
            if pixel.is_none() {
                continue;
            }
            let x = index % self.width;
            let y = index / self.width;
            min_x = min_x.min(x);
            min_y = min_y.min(y);
            max_x = max_x.max(x);
            max_y = max_y.max(y);
            found = true;
        }
        found.then_some((min_x, min_y, max_x - min_x + 1, max_y - min_y + 1))
    }

    fn ink_size(&self) -> Option<(usize, usize)> {
        self.ink_bounds()
            .map(|(_, _, width, height)| (width, height))
    }

    fn align_ink_top(&mut self, target_top: usize) -> Result<()> {
        let (_, current_top, _, ink_height) = self
            .ink_bounds()
            .context("cannot align an empty graphic canvas")?;
        let target_top = target_top.min(self.height - ink_height);
        if target_top == current_top {
            return Ok(());
        }
        let mut aligned = vec![None; self.pixels.len()];
        for (index, pixel) in self.pixels.iter().copied().enumerate() {
            let Some(color) = pixel else {
                continue;
            };
            let x = index % self.width;
            let y = index / self.width;
            let shifted_y = if target_top < current_top {
                y.checked_sub(current_top - target_top)
            } else {
                y.checked_add(target_top - current_top)
            }
            .context("graphic ink alignment moved a pixel outside the canvas")?;
            ensure!(
                shifted_y < self.height,
                "graphic ink alignment moved a pixel outside the canvas"
            );
            aligned[shifted_y * self.width + x] = Some(color);
        }
        self.pixels = aligned;
        Ok(())
    }

    #[cfg(test)]
    fn draw_centered_lines(
        &mut self,
        lines: &[String],
        scale: usize,
        region_y: usize,
        region_height: usize,
        foreground: u8,
    ) -> Result<()> {
        self.draw_centered_lines_with_font(
            lines,
            scale,
            region_y,
            region_height,
            FontRole::Body,
            foreground,
        )
    }

    fn draw_centered_lines_with_font(
        &mut self,
        lines: &[String],
        scale: usize,
        region_y: usize,
        region_height: usize,
        font_role: FontRole,
        foreground: u8,
    ) -> Result<()> {
        ensure!(
            scale > 0 && !lines.is_empty(),
            "invalid graphic text layout"
        );
        let cell_width = GLYPH_WIDTH * scale;
        let cell_height = GLYPH_HEIGHT * scale;
        let text_height = lines
            .len()
            .checked_mul(cell_height)
            .context("graphic text height overflow")?;
        ensure!(
            text_height <= region_height && region_y + region_height <= self.height,
            "graphic text does not fit its vertical region"
        );
        let origin_y = region_y + (region_height - text_height) / 2;
        for (line_index, line) in lines.iter().enumerate() {
            let characters = line.chars().collect::<Vec<_>>();
            let text_width = characters
                .len()
                .checked_mul(cell_width)
                .context("graphic text width overflow")?;
            ensure!(
                text_width <= self.width,
                "graphic line {line:?} needs {text_width} pixels but has {}",
                self.width
            );
            let origin_x = (self.width - text_width) / 2;
            for (character_index, character) in characters.into_iter().enumerate() {
                if character == ' ' {
                    continue;
                }
                let glyph =
                    rasterize_character_for_role(character, font_role).with_context(|| {
                        format!("could not rasterize graphic character {character:?}")
                    })?;
                self.draw_glyph(
                    &glyph,
                    origin_x + character_index * cell_width,
                    origin_y + line_index * cell_height,
                    scale,
                    foreground,
                );
            }
        }
        Ok(())
    }

    fn draw_native_centered_lines(
        &mut self,
        lines: &[String],
        font_role: FontRole,
        font_size: u16,
        advance: usize,
        foreground: u8,
    ) -> Result<()> {
        ensure!(
            !lines.is_empty() && advance > 0,
            "invalid native graphic text layout"
        );
        ensure!(
            self.height.is_multiple_of(lines.len()),
            "native graphic text lines do not divide the canvas height"
        );
        let line_height = self.height / lines.len();
        for (line_index, line) in lines.iter().enumerate() {
            let characters = line.chars().collect::<Vec<_>>();
            let text_width = characters
                .len()
                .checked_mul(advance)
                .context("native graphic text width overflow")?;
            ensure!(
                text_width <= self.width,
                "native graphic line {line:?} needs {text_width} pixels but has {}",
                self.width
            );
            let origin_x = (self.width - text_width) / 2;
            let origin_y = line_index * line_height;
            for (character_index, character) in characters.into_iter().enumerate() {
                if character == ' ' {
                    continue;
                }
                let glyph = rasterize_character_in_cell(
                    character,
                    font_role,
                    font_size,
                    advance,
                    line_height,
                )
                .with_context(|| {
                    format!("could not rasterize native graphic character {character:?}")
                })?;
                for glyph_y in 0..line_height {
                    for glyph_x in 0..advance {
                        if glyph[glyph_y * advance + glyph_x] {
                            let x = origin_x + character_index * advance + glyph_x;
                            let y = origin_y + glyph_y;
                            self.pixels[y * self.width + x] = Some(foreground);
                        }
                    }
                }
            }
        }
        Ok(())
    }

    fn add_outline(&mut self, color: u8) {
        self.add_outline_where(color, |_, _, _, _| true);
    }

    fn add_outline_where(
        &mut self,
        color: u8,
        accepts: impl Fn(usize, usize, usize, usize) -> bool,
    ) {
        let fill = self.pixels.clone();
        for y in 0..self.height {
            for x in 0..self.width {
                if fill[y * self.width + x].is_none() {
                    continue;
                }
                for offset_y in -1_i32..=1 {
                    for offset_x in -1_i32..=1 {
                        let outline_x = x as i32 + offset_x;
                        let outline_y = y as i32 + offset_y;
                        if outline_x < 0
                            || outline_y < 0
                            || outline_x >= self.width as i32
                            || outline_y >= self.height as i32
                        {
                            continue;
                        }
                        let outline_x = outline_x as usize;
                        let outline_y = outline_y as usize;
                        if !accepts(x, y, outline_x, outline_y) {
                            continue;
                        }
                        let index = outline_y * self.width + outline_x;
                        if fill[index].is_none() && self.pixels[index].is_none() {
                            self.pixels[index] = Some(color);
                        }
                    }
                }
            }
        }
    }

    fn draw_glyph(&mut self, glyph: &[u8; 32], x: usize, y: usize, scale: usize, foreground: u8) {
        for source_y in 0..GLYPH_HEIGHT {
            for source_x in 0..GLYPH_WIDTH {
                if glyph[source_y * 2 + source_x / 8] & (0x80 >> (source_x % 8)) == 0 {
                    continue;
                }
                for target_y in y + source_y * scale..y + (source_y + 1) * scale {
                    for target_x in x + source_x * scale..x + (source_x + 1) * scale {
                        self.pixels[target_y * self.width + target_x] = Some(foreground);
                    }
                }
            }
        }
    }
}

fn encode_canvas_cells(
    canvas: &TextCanvas,
    spec: &GraphicSurfaceSpec,
    allocator: &mut TileAllocator,
) -> Result<Vec<u8>> {
    let mut cells = Vec::new();
    for tile_y in 0..spec.height {
        for tile_x in 0..spec.width {
            let record = encode_canvas_tile(canvas, tile_x, tile_y);
            let tile = match spec.format {
                DescriptorFormat::ByteCells {
                    transparent: Some(transparent),
                } if record.iter().all(|byte| *byte == 0) => transparent,
                _ => allocator.allocate(record)?,
            };
            match spec.format {
                DescriptorFormat::ByteCells { .. } => cells.push(tile),
                DescriptorFormat::WordCells => {
                    cells.push(tile);
                    cells.push(1);
                }
            }
        }
    }
    Ok(cells)
}

fn encode_canvas_tile(canvas: &TextCanvas, tile_x: usize, tile_y: usize) -> [u8; BYTES_PER_TILE] {
    let mut record = [0u8; BYTES_PER_TILE];
    for local_y in 0..TILE_HEIGHT {
        for local_x in 0..TILE_WIDTH {
            let x = tile_x * TILE_WIDTH + local_x;
            let y = tile_y * TILE_HEIGHT + local_y;
            let Some(color) = canvas.pixels[y * canvas.width + x] else {
                continue;
            };
            let byte = local_y * 2 + local_x / 8;
            let bit = 0x80 >> (local_x % 8);
            record[byte] |= bit;
            for plane in 0..4 {
                if color & (1 << plane) != 0 {
                    record[(plane + 1) * BYTES_PER_PLANE + byte] |= bit;
                }
            }
        }
    }
    record
}

fn verify_canvas_readback(
    atlas: &[u8],
    canvas: &TextCanvas,
    spec: &GraphicSurfaceSpec,
    cells: &[u8],
) -> Result<()> {
    let cell_size = match spec.format {
        DescriptorFormat::ByteCells { .. } => 1,
        DescriptorFormat::WordCells => 2,
    };
    ensure!(
        cells.len() == spec.width * spec.height * cell_size,
        "{} has the wrong replacement descriptor length",
        spec.id
    );
    for (cell_index, cell) in cells.chunks_exact(cell_size).enumerate() {
        let expected = encode_canvas_tile(canvas, cell_index % spec.width, cell_index / spec.width);
        let tile = match spec.format {
            DescriptorFormat::ByteCells {
                transparent: Some(transparent),
            } if cell[0] == transparent => {
                ensure!(
                    expected.iter().all(|byte| *byte == 0),
                    "{} cell {cell_index} made visible pixels transparent",
                    spec.id
                );
                continue;
            }
            DescriptorFormat::ByteCells { .. } => cell[0],
            DescriptorFormat::WordCells => {
                ensure!(
                    cell[1] == 1,
                    "{} cell {cell_index} changed its word-cell attribute",
                    spec.id
                );
                cell[0]
            }
        };
        let start = usize::from(tile) * BYTES_PER_TILE;
        let actual = atlas
            .get(start..start + BYTES_PER_TILE)
            .context("translated graphic tile is outside the atlas")?;
        ensure!(
            matches!(spec.mask_composition, MaskComposition::RenderedGlyphsOnly)
                && actual == expected,
            "{} cell {cell_index} differs after descriptor/atlas readback",
            spec.id
        );
    }
    Ok(())
}

struct ConfigurationMiniAtlasInstall {
    program: Vec<u8>,
    fixed_expected_write_count: usize,
}

fn install_configuration_mini_atlas(
    program: &[u8],
    compact_atlas: &[u8],
) -> Result<ConfigurationMiniAtlasInstall> {
    ensure!(
        compact_atlas.len().is_multiple_of(BYTES_PER_TILE)
            && compact_atlas.len() <= ATLAS_DECODED_SIZE,
        "configuration mini atlas has invalid tile-record geometry"
    );
    let initial_wrapper_file_offset = program.len();
    let initial_wrapper_runtime_offset = runtime_offset(initial_wrapper_file_offset)?;
    let provisional_initial =
        assemble_configuration_initial_screen_wrapper(initial_wrapper_runtime_offset, 0, 0, 0)?;
    let selected_draw_file_offset = initial_wrapper_file_offset
        .checked_add(provisional_initial.len())
        .context("selected configuration draw stub offset overflow")?;
    let selected_draw_runtime_offset = runtime_offset(selected_draw_file_offset)?;
    let selected_draw = assemble_configuration_state_draw_stub(selected_draw_runtime_offset, true)?;
    let normal_draw_file_offset = selected_draw_file_offset
        .checked_add(selected_draw.len())
        .context("normal configuration draw stub offset overflow")?;
    let normal_draw_runtime_offset = runtime_offset(normal_draw_file_offset)?;
    let normal_draw = assemble_configuration_state_draw_stub(normal_draw_runtime_offset, false)?;
    let segment_word_file_offset = normal_draw_file_offset
        .checked_add(normal_draw.len())
        .context("configuration mini-atlas segment word offset overflow")?;
    let segment_word_runtime_offset = runtime_offset(segment_word_file_offset)?;
    let atlas_file_offset = align_up(
        segment_word_file_offset
            .checked_add(2)
            .context("configuration mini-atlas header overflow")?,
        16,
    )?;
    let atlas_segment_delta = u16::try_from(
        atlas_file_offset
            .checked_add(COM_LOAD_ORIGIN)
            .context("configuration mini-atlas segment overflow")?
            / 16,
    )
    .context("configuration mini-atlas segment delta exceeds 16 bits")?;
    let initial_wrapper = assemble_configuration_initial_screen_wrapper(
        initial_wrapper_runtime_offset,
        segment_word_runtime_offset,
        atlas_segment_delta,
        normal_draw_runtime_offset,
    )?;
    ensure!(
        initial_wrapper.len() == provisional_initial.len(),
        "configuration initial wrapper changed size after placement"
    );
    let initial_screen_hijack = assemble_configuration_call_hijack(
        CONFIGURATION_INITIAL_SCREEN_CALL_FILE_OFFSET + COM_LOAD_ORIGIN,
        initial_wrapper_runtime_offset,
    )?;
    ensure!(
        initial_screen_hijack.len() == 3,
        "configuration initial-screen hijack is not length preserving"
    );

    let mut writes = vec![FixedRangeExpectedWrite {
        owner: "configuration-source-scale-initial-screen",
        purpose: "redraw both configuration pages from the appended mini atlas",
        offset: CONFIGURATION_INITIAL_SCREEN_CALL_FILE_OFFSET,
        expected_source: vec![0xe8, 0x61, 0x17],
        replacement: initial_screen_hijack,
    }];
    for offset in CONFIGURATION_SELECTED_SEGMENT_LOAD_FILE_OFFSETS {
        writes.push(FixedRangeExpectedWrite {
            owner: "configuration-source-scale-selected-redraw",
            purpose: "select the appended mini atlas for selected-state object redraws",
            offset: offset + 2,
            expected_source: CONFIGURATION_SELECTED_SEGMENT_RUNTIME_OFFSET
                .to_le_bytes()
                .to_vec(),
            replacement: segment_word_runtime_offset.to_le_bytes().to_vec(),
        });
    }
    for offset in CONFIGURATION_NORMAL_SEGMENT_LOAD_FILE_OFFSETS {
        writes.push(FixedRangeExpectedWrite {
            owner: "configuration-source-scale-normal-redraw",
            purpose: "select the appended mini atlas for normal-state object redraws",
            offset: offset + 2,
            expected_source: CONFIGURATION_NORMAL_SEGMENT_RUNTIME_OFFSET
                .to_le_bytes()
                .to_vec(),
            replacement: segment_word_runtime_offset.to_le_bytes().to_vec(),
        });
    }
    for (offset, expected_source) in CONFIGURATION_SELECTED_DRAW_CALLS {
        writes.push(FixedRangeExpectedWrite {
            owner: "configuration-source-scale-selected-palette",
            purpose: "prepare selected palette planes before drawing one configuration object",
            offset,
            expected_source: expected_source.to_vec(),
            replacement: assemble_configuration_call_hijack(
                offset + COM_LOAD_ORIGIN,
                selected_draw_runtime_offset,
            )?,
        });
    }
    for (offset, expected_source) in CONFIGURATION_NORMAL_DRAW_CALLS {
        writes.push(FixedRangeExpectedWrite {
            owner: "configuration-source-scale-normal-palette",
            purpose: "prepare normal palette planes before drawing one configuration object",
            offset,
            expected_source: expected_source.to_vec(),
            replacement: assemble_configuration_call_hijack(
                offset + COM_LOAD_ORIGIN,
                normal_draw_runtime_offset,
            )?,
        });
    }
    let mut updated = apply_fixed_range_expected_writes(program, &writes)?;
    updated.extend_from_slice(&initial_wrapper);
    updated.extend_from_slice(&selected_draw);
    updated.extend_from_slice(&normal_draw);
    updated.extend_from_slice(&[0, 0]);
    updated.resize(atlas_file_offset, 0);
    updated.extend_from_slice(compact_atlas);
    ensure!(
        updated.len() + COM_LOAD_ORIGIN <= 0x1_0000,
        "configuration mini atlas exceeds the OPENING.COM segment"
    );
    ensure!(
        updated
            .get(initial_wrapper_file_offset..initial_wrapper_file_offset + initial_wrapper.len())
            == Some(initial_wrapper.as_slice())
            && updated
                .get(selected_draw_file_offset..selected_draw_file_offset + selected_draw.len())
                == Some(selected_draw.as_slice())
            && updated.get(normal_draw_file_offset..normal_draw_file_offset + normal_draw.len())
                == Some(normal_draw.as_slice())
            && updated.get(segment_word_file_offset..segment_word_file_offset + 2) == Some(&[0, 0])
            && updated.get(atlas_file_offset..) == Some(compact_atlas),
        "configuration runtime payload differs after append"
    );
    Ok(ConfigurationMiniAtlasInstall {
        program: updated,
        fixed_expected_write_count: writes.len(),
    })
}

fn assemble_configuration_initial_screen_wrapper(
    origin: u16,
    segment_word: u16,
    atlas_segment_delta: u16,
    normal_draw_entry: u16,
) -> Result<Vec<u8>> {
    let mut assembler = Assembler::new();
    assembler
        .emit(Instruction::Pusha)
        .emit(Instruction::Push {
            src: segment_operand(SegmentRegister::DS),
        })
        .emit(Instruction::Mov {
            dest: reg16_operand(Register16::AX),
            src: segment_operand(SegmentRegister::CS),
        })
        .emit(Instruction::Add {
            dest: reg16_operand(Register16::AX),
            src: imm16_operand(atlas_segment_delta),
        })
        .emit(Instruction::Mov {
            dest: direct_memory_operand(Some(SegmentRegister::CS), segment_word, OperandSize::Word),
            src: reg16_operand(Register16::AX),
        })
        .emit_call_near("original_initial_screen")
        .emit_call_near("draw_normal_page")
        .emit(Instruction::Mov {
            dest: reg8_operand(Register8::AL),
            src: direct_memory_operand(
                Some(SegmentRegister::CS),
                CONFIGURATION_PAGE_STATE_RUNTIME_OFFSET,
                OperandSize::Byte,
            ),
        })
        .emit(Instruction::Xor {
            dest: reg8_operand(Register8::AL),
            src: imm8_operand(1),
        })
        .emit(Instruction::OutAl {
            port: PortAddress::Imm8(0xa6),
        })
        .emit_call_near("draw_normal_page")
        .emit(Instruction::Mov {
            dest: reg8_operand(Register8::AL),
            src: direct_memory_operand(
                Some(SegmentRegister::CS),
                CONFIGURATION_PAGE_STATE_RUNTIME_OFFSET,
                OperandSize::Byte,
            ),
        })
        .emit(Instruction::OutAl {
            port: PortAddress::Imm8(0xa6),
        })
        .emit(Instruction::Pop {
            dest: segment_operand(SegmentRegister::DS),
        })
        .emit(Instruction::Popa)
        .emit(Instruction::Ret { pop: 0 })
        .label("draw_normal_page")
        .emit(Instruction::Mov {
            dest: reg16_operand(Register16::SI),
            src: imm16_operand(CONFIGURATION_NORMAL_OBJECT_TABLE_RUNTIME_OFFSET),
        })
        .emit(Instruction::Mov {
            dest: reg16_operand(Register16::CX),
            src: imm16_operand(CONFIGURATION_OBJECT_COUNT as u16),
        })
        .label("draw_object")
        .emit(Instruction::Mov {
            dest: reg16_operand(Register16::AX),
            src: direct_memory_operand(
                Some(SegmentRegister::CS),
                CONFIGURATION_BACKGROUND_SEGMENT_RUNTIME_OFFSET,
                OperandSize::Word,
            ),
        })
        .emit(Instruction::Mov {
            dest: segment_operand(SegmentRegister::DS),
            src: reg16_operand(Register16::AX),
        })
        .emit(Instruction::Push {
            src: reg16_operand(Register16::CX),
        })
        .emit_call_near("restore_background")
        .emit(Instruction::Mov {
            dest: reg16_operand(Register16::AX),
            src: direct_memory_operand(Some(SegmentRegister::CS), segment_word, OperandSize::Word),
        })
        .emit(Instruction::Mov {
            dest: segment_operand(SegmentRegister::DS),
            src: reg16_operand(Register16::AX),
        })
        .emit_call_near("draw_normal_object")
        .emit(Instruction::Pop {
            dest: reg16_operand(Register16::CX),
        })
        .emit(Instruction::Add {
            dest: reg16_operand(Register16::SI),
            src: imm16_operand(CONFIGURATION_OBJECT_RECORD_SIZE as u16),
        })
        .emit_loop(LoopCondition::Always, "draw_object")
        .emit(Instruction::Ret { pop: 0 })
        .label("original_initial_screen")
        .emit(Instruction::Push {
            src: imm16_operand(CONFIGURATION_ORIGINAL_INITIAL_SCREEN_RUNTIME_OFFSET),
        })
        .emit(Instruction::Ret { pop: 0 })
        .label("restore_background")
        .emit(Instruction::Push {
            src: imm16_operand(CONFIGURATION_BACKGROUND_RESTORE_RUNTIME_OFFSET),
        })
        .emit(Instruction::Ret { pop: 0 })
        .label("draw_normal_object")
        .emit(Instruction::Push {
            src: imm16_operand(normal_draw_entry),
        })
        .emit(Instruction::Ret { pop: 0 });
    assemble_v30_at(&assembler, origin, "configuration mini-atlas wrapper")
}

fn assemble_configuration_state_draw_stub(origin: u16, selected: bool) -> Result<Vec<u8>> {
    let mut assembler = Assembler::new();
    assembler
        .emit_call_near("prepare_tiles")
        .emit(Instruction::Push {
            src: imm16_operand(CONFIGURATION_OBJECT_DRAW_RUNTIME_OFFSET),
        })
        .emit(Instruction::Ret { pop: 0 })
        .label("prepare_tiles")
        .emit(Instruction::Pusha)
        .emit(Instruction::Push {
            src: segment_operand(SegmentRegister::ES),
        })
        .emit(Instruction::Push {
            src: segment_operand(SegmentRegister::DS),
        })
        .emit(Instruction::Pop {
            dest: segment_operand(SegmentRegister::ES),
        })
        .emit(Instruction::Xor {
            dest: reg16_operand(Register16::AX),
            src: reg16_operand(Register16::AX),
        })
        .emit(Instruction::Mov {
            dest: reg8_operand(Register8::AL),
            src: based_memory_operand(
                Some(SegmentRegister::CS),
                EffectiveAddressBase::Si,
                6,
                OperandSize::Byte,
            ),
        })
        .emit(Instruction::Mov {
            dest: reg8_operand(Register8::CL),
            src: based_memory_operand(
                Some(SegmentRegister::CS),
                EffectiveAddressBase::Si,
                7,
                OperandSize::Byte,
            ),
        })
        .emit(Instruction::Mul {
            src: reg8_operand(Register8::CL),
        })
        .emit(Instruction::Mov {
            dest: reg16_operand(Register16::BP),
            src: reg16_operand(Register16::AX),
        })
        .emit(Instruction::Mov {
            dest: reg16_operand(Register16::BX),
            src: based_memory_operand(
                Some(SegmentRegister::CS),
                EffectiveAddressBase::Si,
                0,
                OperandSize::Word,
            ),
        })
        .emit(Instruction::Add {
            dest: reg16_operand(Register16::BX),
            src: imm16_operand(2),
        })
        .label("tile")
        .emit(Instruction::Mov {
            dest: reg8_operand(Register8::AL),
            src: based_memory_operand(
                Some(SegmentRegister::CS),
                EffectiveAddressBase::Bx,
                0,
                OperandSize::Byte,
            ),
        })
        .emit(Instruction::Inc {
            dest: reg16_operand(Register16::BX),
        })
        .emit(Instruction::Xor {
            dest: reg8_operand(Register8::AH),
            src: reg8_operand(Register8::AH),
        })
        .emit(Instruction::Mov {
            dest: reg16_operand(Register16::DI),
            src: reg16_operand(Register16::AX),
        });
    for _ in 0..5 {
        assembler.emit(Instruction::Shl {
            dest: reg16_operand(Register16::AX),
            count: ShiftCount::One,
        });
    }
    for _ in 0..7 {
        assembler.emit(Instruction::Shl {
            dest: reg16_operand(Register16::DI),
            count: ShiftCount::One,
        });
    }
    assembler
        .emit(Instruction::Add {
            dest: reg16_operand(Register16::DI),
            src: reg16_operand(Register16::AX),
        })
        .emit(Instruction::Mov {
            dest: reg16_operand(Register16::DX),
            src: reg16_operand(Register16::DI),
        });
    if selected {
        for destination in [BYTES_PER_PLANE, BYTES_PER_PLANE * 2] {
            assembler
                .emit(Instruction::Mov {
                    dest: reg16_operand(Register16::SI),
                    src: reg16_operand(Register16::DX),
                })
                .emit(Instruction::Mov {
                    dest: reg16_operand(Register16::DI),
                    src: reg16_operand(Register16::DX),
                })
                .emit(Instruction::Add {
                    dest: reg16_operand(Register16::DI),
                    src: imm16_operand(destination as u16),
                })
                .emit(Instruction::Mov {
                    dest: reg16_operand(Register16::CX),
                    src: imm16_operand((BYTES_PER_PLANE / 2) as u16),
                })
                .emit(Instruction::Rep(Box::new(Instruction::Movsw)));
        }
        assembler
            .emit(Instruction::Mov {
                dest: reg16_operand(Register16::SI),
                src: reg16_operand(Register16::DX),
            })
            .emit(Instruction::Add {
                dest: reg16_operand(Register16::SI),
                src: imm16_operand((BYTES_PER_PLANE * 4) as u16),
            })
            .emit(Instruction::Mov {
                dest: reg16_operand(Register16::DI),
                src: reg16_operand(Register16::DX),
            })
            .emit(Instruction::Add {
                dest: reg16_operand(Register16::DI),
                src: imm16_operand((BYTES_PER_PLANE * 3) as u16),
            })
            .emit(Instruction::Mov {
                dest: reg16_operand(Register16::CX),
                src: imm16_operand((BYTES_PER_PLANE / 2) as u16),
            })
            .emit(Instruction::Rep(Box::new(Instruction::Movsw)));
    } else {
        assembler
            .emit(Instruction::Xor {
                dest: reg16_operand(Register16::AX),
                src: reg16_operand(Register16::AX),
            })
            .emit(Instruction::Mov {
                dest: reg16_operand(Register16::DI),
                src: reg16_operand(Register16::DX),
            })
            .emit(Instruction::Add {
                dest: reg16_operand(Register16::DI),
                src: imm16_operand(BYTES_PER_PLANE as u16),
            })
            .emit(Instruction::Mov {
                dest: reg16_operand(Register16::CX),
                src: imm16_operand((BYTES_PER_PLANE / 2) as u16),
            })
            .emit(Instruction::Rep(Box::new(Instruction::Stosw)))
            .emit(Instruction::Mov {
                dest: reg16_operand(Register16::SI),
                src: reg16_operand(Register16::DX),
            })
            .emit(Instruction::Add {
                dest: reg16_operand(Register16::SI),
                src: imm16_operand((BYTES_PER_PLANE * 4) as u16),
            })
            .emit(Instruction::Mov {
                dest: reg16_operand(Register16::DI),
                src: reg16_operand(Register16::DX),
            })
            .emit(Instruction::Add {
                dest: reg16_operand(Register16::DI),
                src: imm16_operand((BYTES_PER_PLANE * 2) as u16),
            })
            .emit(Instruction::Mov {
                dest: reg16_operand(Register16::CX),
                src: imm16_operand((BYTES_PER_PLANE / 2) as u16),
            })
            .emit(Instruction::Rep(Box::new(Instruction::Movsw)))
            .emit(Instruction::Xor {
                dest: reg16_operand(Register16::AX),
                src: reg16_operand(Register16::AX),
            })
            .emit(Instruction::Mov {
                dest: reg16_operand(Register16::DI),
                src: reg16_operand(Register16::DX),
            })
            .emit(Instruction::Add {
                dest: reg16_operand(Register16::DI),
                src: imm16_operand((BYTES_PER_PLANE * 3) as u16),
            })
            .emit(Instruction::Mov {
                dest: reg16_operand(Register16::CX),
                src: imm16_operand((BYTES_PER_PLANE / 2) as u16),
            })
            .emit(Instruction::Rep(Box::new(Instruction::Stosw)));
    }
    assembler
        .emit(Instruction::Dec {
            dest: reg16_operand(Register16::BP),
        })
        .emit_branch(Condition::Ne, "tile")
        .emit(Instruction::Pop {
            dest: segment_operand(SegmentRegister::ES),
        })
        .emit(Instruction::Popa)
        .emit(Instruction::Ret { pop: 0 });
    assemble_v30_at(
        &assembler,
        origin,
        if selected {
            "selected configuration palette preparation"
        } else {
            "normal configuration palette preparation"
        },
    )
}

fn assemble_configuration_call_hijack(origin: usize, target: u16) -> Result<Vec<u8>> {
    let origin = u16::try_from(origin).context("configuration hijack origin exceeds 16 bits")?;
    let mut assembler = Assembler::new();
    assembler.emit(Instruction::Call {
        target: CallTarget::Rel16(near_displacement(origin, target)),
    });
    assemble_v30_at(&assembler, origin, "configuration initial-screen hijack")
}

fn assemble_v30_at(assembler: &Assembler, origin: u16, purpose: &str) -> Result<Vec<u8>> {
    assembler
        .assemble(CodeLocation {
            seg: 0,
            off: origin,
        })
        .with_context(|| format!("assemble typed V30 {purpose} at 0x{origin:04X}"))
        .map(|program| program.bytes().to_vec())
}

fn direct_memory_operand(
    segment: Option<SegmentRegister>,
    address: u16,
    size: OperandSize,
) -> Operand {
    Operand::Mem(
        EffectiveAddress::new(
            segment,
            EffectiveAddressBase::Direct,
            EffectiveAddressDisplacement::Absolute(address),
            size,
        )
        .expect("configuration runtime direct address is representable"),
    )
}

fn based_memory_operand(
    segment: Option<SegmentRegister>,
    base: EffectiveAddressBase,
    displacement: i16,
    size: OperandSize,
) -> Operand {
    Operand::Mem(
        EffectiveAddress::new(
            segment,
            base,
            EffectiveAddressDisplacement::Signed(displacement),
            size,
        )
        .expect("configuration runtime based address is representable"),
    )
}

const fn reg8_operand(register: Register8) -> Operand {
    Operand::Reg8(register)
}

const fn reg16_operand(register: Register16) -> Operand {
    Operand::Reg16(register)
}

const fn segment_operand(register: SegmentRegister) -> Operand {
    Operand::Sreg(register)
}

const fn imm8_operand(value: u8) -> Operand {
    Operand::Imm8(value)
}

const fn imm16_operand(value: u16) -> Operand {
    Operand::Imm16(value)
}

const fn near_displacement(origin: u16, target: u16) -> i16 {
    target.wrapping_sub(origin.wrapping_add(3)) as i16
}

fn runtime_offset(file_offset: usize) -> Result<u16> {
    u16::try_from(
        file_offset
            .checked_add(COM_LOAD_ORIGIN)
            .context("COM runtime offset overflow")?,
    )
    .context("COM runtime offset exceeds 16 bits")
}

fn align_up(value: usize, alignment: usize) -> Result<usize> {
    ensure!(
        alignment.is_power_of_two(),
        "alignment must be a power of two"
    );
    value
        .checked_add(alignment - 1)
        .map(|rounded| rounded & !(alignment - 1))
        .context("alignment overflow")
}

struct TileAllocator {
    available: Vec<u8>,
    records: BTreeMap<Vec<u8>, u8>,
}

impl TileAllocator {
    fn new(available: Vec<u8>) -> Self {
        Self {
            available,
            records: BTreeMap::new(),
        }
    }

    fn allocate(&mut self, record: [u8; BYTES_PER_TILE]) -> Result<u8> {
        if let Some(tile) = self.records.get(&record[..]) {
            return Ok(*tile);
        }
        let index = self.records.len();
        let tile = self.available.get(index).copied().with_context(|| {
            format!(
                "graphic translation needs tile record {} but owns only {} records",
                index + 1,
                self.available.len(),
            )
        })?;
        self.records.insert(record.to_vec(), tile);
        Ok(tile)
    }

    fn write_tiles(&self, atlas: &mut [u8]) -> Result<()> {
        ensure!(
            atlas.len() == ATLAS_DECODED_SIZE,
            "graphic translation atlas has the wrong size"
        );
        for (record, tile) in &self.records {
            let start = usize::from(*tile) * BYTES_PER_TILE;
            atlas[start..start + BYTES_PER_TILE].copy_from_slice(record);
        }
        Ok(())
    }

    fn compact_size(&self) -> usize {
        self.records
            .values()
            .copied()
            .max()
            .map_or(BYTES_PER_TILE, |tile| {
                (usize::from(tile) + 1) * BYTES_PER_TILE
            })
    }
}

#[cfg(test)]
#[path = "graphic_localization_tests.rs"]
mod tests;
