use super::{
    BYTES_PER_PLANE, BYTES_PER_TILE, DescriptorFormat, GraphicSurfaceSpec, MaskComposition,
    OWNER_PLAYER_LABELS, PRESERVED_OWNER_PLAYER_LABEL_TILES, PRESERVED_UNIT_IDS, SurfaceDrawMode,
    SurfaceLayout, TITLE_MENU_SURFACES, TRANSLATED_SURFACES, TextCanvas, TileAllocator,
    encode_canvas_cells, encode_canvas_tile, load_graphic_translation_catalog,
    recolor_configuration_mini_atlas, render_surface, source_text_colors, verify_canvas_readback,
};
use crate::font::FontRole;
use crate::localization::sha256_hex;
use crate::masked_tile::decode_pixel;

#[test]
#[ignore = "requires assets/translations/graphic-text.json"]
fn tracked_graphic_catalog_owns_every_translation_and_preservation_unit() {
    let catalog = load_graphic_translation_catalog().unwrap();

    let ids = catalog
        .units
        .iter()
        .map(|unit| unit.id.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    let expected_ids = TRANSLATED_SURFACES
        .iter()
        .map(|spec| spec.id)
        .chain(PRESERVED_UNIT_IDS)
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(ids, expected_ids);
    for player in 1..=3 {
        assert!(ids.contains(format!("selection-owner-player-{player}-label").as_str()));
    }
    assert!(ids.contains("config-heading"));
    assert!(ids.contains("selection-player-frame"));
    assert!(ids.contains("config-return"));
    assert!(!ids.contains("opening-tournament-card"));
}

#[test]
fn english_title_menu_states_are_preserved_instead_of_rebuilt() {
    let title_ids = TITLE_MENU_SURFACES
        .iter()
        .map(|spec| spec.id)
        .collect::<std::collections::BTreeSet<_>>();

    assert_eq!(
        title_ids,
        std::collections::BTreeSet::from([
            "title-game-start-normal",
            "title-config-normal",
            "title-game-start-selected",
            "title-config-selected",
        ])
    );
    assert!(title_ids.iter().all(|id| PRESERVED_UNIT_IDS.contains(id)));
    assert!(
        TRANSLATED_SURFACES
            .iter()
            .all(|spec| !title_ids.contains(spec.id))
    );
}

#[test]
#[ignore = "requires the ending graphic candidates in assets/graphics/ending/candidates"]
fn generated_ending_candidates_are_fixed_nonapproved_inputs() {
    let read = |name: &str| {
        crate::private_input::read_bytes(&format!("graphics/ending/candidates/{name}")).unwrap()
    };
    let manifest: serde_json::Value = serde_json::from_slice(read("manifest.json")).unwrap();
    let candidates = manifest["candidates"].as_array().unwrap();
    assert_eq!(candidates.len(), 2);
    for (candidate, bytes) in candidates.iter().zip([
        read("ending-finished-imagegen.png"),
        read("ending-play-again-imagegen.png"),
    ]) {
        assert_eq!(candidate["sha256"], sha256_hex(bytes));
        assert!(
            candidate["status"] == "needs_human_review"
                || candidate["status"] == "needs_background_extraction_and_human_review"
        );
    }
}

#[test]
#[ignore = "requires assets/translations/graphic-text.json"]
fn english_owner_player_labels_are_preserved_instead_of_rebuilt() {
    let catalog = load_graphic_translation_catalog().unwrap();
    let observed_tiles = OWNER_PLAYER_LABELS
        .iter()
        .flat_map(|label| label.label_tiles)
        .collect::<std::collections::BTreeSet<_>>();
    let expected_tiles = PRESERVED_OWNER_PLAYER_LABEL_TILES
        .into_iter()
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(observed_tiles, expected_tiles);
    for label in OWNER_PLAYER_LABELS {
        let unit = catalog
            .units
            .iter()
            .find(|unit| unit.id == label.id)
            .unwrap();
        assert_eq!(unit.status, "preserve_source");
        assert!(unit.ko_lines.is_empty());
        assert!(PRESERVED_UNIT_IDS.contains(&label.id));
    }
}

#[test]
fn configuration_text_uses_menu_object_descriptors_instead_of_opening_card_page_grids() {
    let configuration_surfaces = TRANSLATED_SURFACES
        .iter()
        .filter(|spec| matches!(spec.target_asset, "CFG.DAT" | "CFG_N.DAT"))
        .collect::<Vec<_>>();

    assert_eq!(configuration_surfaces.len(), 20);
    assert!(
        configuration_surfaces
            .iter()
            .all(|spec| spec.foreground_source_asset == spec.target_asset)
    );
    assert!(
        configuration_surfaces
            .iter()
            .all(|spec| matches!(spec.mask_composition, MaskComposition::RenderedGlyphsOnly))
    );
    assert!(
        configuration_surfaces
            .iter()
            .all(|spec| matches!(spec.draw_mode, SurfaceDrawMode::MaskCoverage))
    );
    assert!(configuration_surfaces.iter().all(|spec| matches!(
        spec.layout,
        SurfaceLayout::NativeCenteredText {
            font_role: FontRole::UtilityLettering,
            outline: true,
            ..
        }
    )));
    assert!(
        configuration_surfaces
            .iter()
            .all(|spec| spec.minimum_ink_height.is_some())
    );
    let opening_page_offsets = [
        0x3914, 0x393a, 0x3960, 0x3986, 0x39ac, 0x39d2, 0x39f8, 0x3a1e,
    ];
    assert!(
        configuration_surfaces
            .iter()
            .all(|spec| !opening_page_offsets.contains(&spec.descriptor_offset))
    );
    for id in [
        "config-player-selection",
        "config-quest-setting",
        "config-return",
        "config-one-player",
        "config-two-players",
        "config-all-players",
        "config-quest-count-3",
        "config-quest-count-5",
        "config-quest-count-7",
        "config-quest-count-all",
    ] {
        assert_eq!(
            configuration_surfaces
                .iter()
                .filter(|spec| spec.id == id)
                .count(),
            2
        );
    }
}

#[test]
fn configuration_mini_atlas_recoloring_preserves_shape_and_assigns_state_palettes() {
    let mut atlas = vec![0; 256 * BYTES_PER_TILE];
    atlas[0] = 0xc0;
    atlas[4 * BYTES_PER_PLANE] = 0x80;

    let selected = recolor_configuration_mini_atlas(&atlas, true).unwrap();
    let normal = recolor_configuration_mini_atlas(&atlas, false).unwrap();

    let selected_fill = decode_pixel(&selected, 0, 0, 0).unwrap();
    let selected_outline = decode_pixel(&selected, 0, 1, 0).unwrap();
    let normal_fill = decode_pixel(&normal, 0, 0, 0).unwrap();
    let normal_outline = decode_pixel(&normal, 0, 1, 0).unwrap();
    assert_eq!((selected_fill.mask, selected_fill.color_index), (true, 15));
    assert_eq!(
        (selected_outline.mask, selected_outline.color_index),
        (true, 3)
    );
    assert_eq!((normal_fill.mask, normal_fill.color_index), (true, 10));
    assert_eq!((normal_outline.mask, normal_outline.color_index), (true, 0));
}

#[test]
fn tile_allocator_deduplicates_records_and_fails_when_owned_capacity_is_exhausted() {
    let mut allocator = TileAllocator::new(vec![3]);
    let first = [1u8; BYTES_PER_TILE];
    let second = [2u8; BYTES_PER_TILE];

    assert_eq!(allocator.allocate(first).unwrap(), 3);
    assert_eq!(allocator.allocate(first).unwrap(), 3);
    assert!(allocator.allocate(second).is_err());
}

#[test]
fn centered_graphic_text_fails_closed_when_it_exceeds_the_canvas() {
    let mut canvas = TextCanvas {
        width: 16,
        height: 16,
        pixels: vec![None; 16 * 16],
    };

    assert!(
        canvas
            .draw_centered_lines(&["두글자".to_owned()], 1, 0, 16, 15)
            .is_err()
    );
}

#[test]
fn graphic_ink_alignment_matches_the_source_top_and_clamps_to_the_canvas() {
    let mut canvas = TextCanvas {
        width: 4,
        height: 8,
        pixels: vec![None; 4 * 8],
    };
    canvas.pixels[6 * 4] = Some(15);
    canvas.pixels[7 * 4] = Some(15);

    canvas.align_ink_top(2).unwrap();
    assert_eq!(canvas.ink_bounds(), Some((0, 2, 1, 2)));

    canvas.align_ink_top(7).unwrap();
    assert_eq!(canvas.ink_bounds(), Some((0, 6, 1, 2)));
}

#[cfg(feature = "analysis")]
#[test]
fn protected_background_check_ignores_only_source_or_localized_ink() {
    let source = [1, 2, 3, 4, 5, 6];
    let localized = [9, 9, 9, 4, 5, 7];

    assert_eq!(
        super::count_changed_rgb_pixels_outside_masks(
            &source,
            &localized,
            &[true, false],
            &[false, false],
        )
        .unwrap(),
        1
    );
    assert_eq!(
        super::count_changed_rgb_pixels_outside_masks(
            &source,
            &localized,
            &[true, false],
            &[false, true],
        )
        .unwrap(),
        0
    );
}

#[test]
#[ignore = "requires assets/translations/graphic-text.json and the font files under assets/fonts"]
fn translated_utility_surfaces_enforce_source_ink_height_without_width_squashing() {
    let catalog = load_graphic_translation_catalog().unwrap();
    for spec in TRANSLATED_SURFACES.iter().filter(|spec| {
        matches!(
            spec.target_asset,
            "CFG.DAT" | "CFG_N.DAT" | "SELECT.DAT" | "C_CHAR1.DAT" | "MAP_CHR.DAT"
        )
    }) {
        let unit = catalog
            .units
            .iter()
            .find(|unit| unit.id == spec.id)
            .unwrap();
        let canvas = render_surface(spec, unit, 15, Some(3), None).unwrap();
        let (ink_width, ink_height) = canvas.ink_size().unwrap();
        assert!(ink_width > 0);
        assert!(ink_height >= spec.minimum_ink_height.unwrap());
        if unit
            .ko_lines
            .iter()
            .flat_map(|line| line.chars())
            .any(|character| ('가'..='힣').contains(&character))
            && let SurfaceLayout::NativeCenteredText {
                font_size, advance, ..
            } = spec.layout
        {
            assert!(advance * 5 >= usize::from(font_size) * 4);
        }
    }
}

#[test]
fn selection_prompts_keep_their_independent_source_height_profiles() {
    let team = TRANSLATED_SURFACES
        .iter()
        .find(|spec| spec.id == "selection-team-prompt")
        .unwrap();
    let owner = TRANSLATED_SURFACES
        .iter()
        .find(|spec| spec.id == "selection-owner-prompt")
        .unwrap();

    assert_eq!(team.minimum_ink_height, Some(48));
    assert!(matches!(
        team.layout,
        SurfaceLayout::NativeCenteredText {
            font_role: FontRole::SelectionPrompt,
            font_size: 55,
            advance: 61,
            outline: true,
        }
    ));
    assert_eq!(owner.minimum_ink_height, Some(40));
    assert!(matches!(
        owner.layout,
        SurfaceLayout::NativeCenteredText {
            font_role: FontRole::SelectionPrompt,
            font_size: 45,
            advance: 50,
            outline: true,
        }
    ));
}

#[test]
fn encoded_mask_distinguishes_palette_zero_ink_from_an_empty_cell() {
    let mut canvas = TextCanvas {
        width: 16,
        height: 16,
        pixels: vec![None; 16 * 16],
    };
    canvas.pixels[0] = Some(0);

    let record = encode_canvas_tile(&canvas, 0, 0);

    assert_eq!(record[0], 0x80);
    assert!(record[32..].iter().all(|byte| *byte == 0));
}

#[test]
fn source_text_colors_are_ranked_by_the_original_nonzero_palette_roles() {
    let mut canvas = TextCanvas {
        width: 16,
        height: 16,
        pixels: vec![None; 16 * 16],
    };
    for x in 0..3 {
        canvas.pixels[x] = Some(15);
    }
    for x in 3..5 {
        canvas.pixels[x] = Some(5);
    }
    let record = encode_canvas_tile(&canvas, 0, 0);
    let mut atlas = vec![0; 256 * BYTES_PER_TILE];
    atlas[..BYTES_PER_TILE].copy_from_slice(&record);

    assert_eq!(source_text_colors(&atlas, &[0]).unwrap(), vec![15, 5]);
}

#[test]
fn descriptor_and_atlas_readback_reconstructs_the_canvas_and_detects_corruption() {
    let spec = GraphicSurfaceSpec {
        id: "readback-test",
        program: "TEST.COM",
        target_asset: "TEST.DAT",
        foreground_source_asset: "TEST.DAT",
        descriptor_offset: 0,
        width: 1,
        height: 1,
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
    };
    let mut canvas = TextCanvas {
        width: 16,
        height: 16,
        pixels: vec![None; 16 * 16],
    };
    canvas.pixels[0] = Some(15);
    let mut allocator = TileAllocator::new(vec![3]);
    let cells = encode_canvas_cells(&canvas, &spec, &mut allocator).unwrap();
    let mut atlas = vec![0; 256 * BYTES_PER_TILE];
    allocator.write_tiles(&mut atlas).unwrap();

    verify_canvas_readback(&atlas, &canvas, &spec, &cells).unwrap();
    atlas[3 * BYTES_PER_TILE] ^= 0x80;
    assert!(verify_canvas_readback(&atlas, &canvas, &spec, &cells).is_err());
}
