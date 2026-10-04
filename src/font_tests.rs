use super::{
    FontRole, font_report, font_report_for_role, rasterize_character_for_role,
    rasterize_character_in_cell,
};

#[test]
#[ignore = "requires the Galmuri14, NeoDunggeunmo and BMJUA font files under assets/fonts (or MADOU456_ASSET_DIR/fonts)"]
fn embedded_fonts_match_their_profiles_and_render_hangul() {
    assert_eq!(
        font_report().unwrap().font_sha256,
        "6fe6c3fe4369e3837ac348431e8670733d67aa4bd550982baa72cc93c81a1c68"
    );
    for (role, expected_hash) in [
        (
            FontRole::Body,
            "6fe6c3fe4369e3837ac348431e8670733d67aa4bd550982baa72cc93c81a1c68",
        ),
        (
            FontRole::UtilityLettering,
            "d61b60eccb731f8ca9c7da582e4a05a94db66b570471809950aa9a7261b941d6",
        ),
        (
            FontRole::SelectionPrompt,
            "e8e6aa8b1b662c7bf0d7f136f29e822e0985176458a6e5d0ba08afc4a5c901a9",
        ),
    ] {
        assert_eq!(
            font_report_for_role(role).unwrap().font_sha256,
            expected_hash
        );
        assert!(
            rasterize_character_for_role('가', role)
                .unwrap()
                .iter()
                .any(|byte| *byte != 0)
        );
    }
}

#[test]
#[ignore = "requires the Galmuri14, NeoDunggeunmo and BMJUA font files under assets/fonts (or MADOU456_ASSET_DIR/fonts)"]
fn display_fonts_render_directly_into_large_surface_cells() {
    let prompt = rasterize_character_in_cell('팀', FontRole::SelectionPrompt, 38, 44, 48).unwrap();
    assert_eq!(prompt.len(), 44 * 48);
    assert!(prompt.iter().filter(|pixel| **pixel).count() > 300);

    let configuration =
        rasterize_character_in_cell('혼', FontRole::UtilityLettering, 30, 36, 48).unwrap();
    assert_eq!(configuration.len(), 36 * 48);
    assert!(configuration.iter().filter(|pixel| **pixel).count() > 100);
}
