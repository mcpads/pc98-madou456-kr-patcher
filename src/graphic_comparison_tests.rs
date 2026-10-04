use super::{encode_bmp, load_and_audit_surface_ledger, side_by_side_rgb};
use crate::graphic_localization::GraphicSurfaceComparisonImage;

#[test]
#[ignore = "requires assets/analysis/graphic-text-surfaces.json and assets/translations/graphic-text.json"]
fn surface_ledger_owns_the_catalog_and_preserves_the_wordmark_and_english_menu() {
    let ledger = load_and_audit_surface_ledger().unwrap();
    assert_eq!(ledger.groups.len(), 6);
    let title = ledger
        .groups
        .iter()
        .find(|group| group.id == "title-wordmark")
        .unwrap();
    assert_eq!(title.tracking_status, "mapped_preserve_source");
    assert!(title.translation_unit_ids.is_empty());
    assert!(title.localized_presentation.contains("byte-for-byte"));
    assert!(
        title
            .current_assessment
            .contains("no title-plane localization write")
    );

    let title_menu = ledger
        .groups
        .iter()
        .find(|group| group.id == "title-menu-lettering")
        .unwrap();
    assert_eq!(title_menu.tracking_status, "mapped_preserve_source");
    assert!(
        title_menu
            .consumer_composition
            .contains("no cell is transparent")
    );
    assert!(title_menu.current_assessment.contains("already English"));
    assert!(title_menu.localized_presentation.contains("preserved"));
}

#[test]
fn comparison_bitmap_keeps_source_left_and_localized_right() {
    let image = GraphicSurfaceComparisonImage {
        id: "fixture".to_owned(),
        unit_id: None,
        source_asset: "FIXTURE.DAT",
        consumer_program: "FIXTURE.COM",
        classification: "fixture",
        width: 1,
        height: 1,
        minimum_ink_height: None,
        source_ink_bounds: Some([0, 0, 1, 1]),
        localized_ink_bounds: Some([0, 0, 1, 1]),
        source_ink_size: Some([1, 1]),
        localized_ink_size: Some([1, 1]),
        source_ink_palette_indices: vec![1],
        localized_ink_palette_indices: vec![2],
        protected_background_pixels_changed: Some(0),
        source_rgb: vec![1, 2, 3],
        localized_rgb: vec![4, 5, 6],
    };
    let pair = side_by_side_rgb(&image).unwrap();
    assert_eq!(&pair[..3], &[1, 2, 3]);
    assert!(pair[3..3 + 16 * 3].iter().all(|byte| *byte == 0x30));
    assert_eq!(&pair[pair.len() - 3..], &[4, 5, 6]);

    let bmp = encode_bmp(18, 1, &pair).unwrap();
    assert_eq!(&bmp[..2], b"BM");
    assert_eq!(u32::from_le_bytes(bmp[18..22].try_into().unwrap()), 18);
    assert_eq!(u32::from_le_bytes(bmp[22..26].try_into().unwrap()), 1);
}
