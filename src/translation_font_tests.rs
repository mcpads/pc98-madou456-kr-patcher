use std::collections::BTreeSet;

use super::{
    TRANSLATION_GLYPH_CAPACITY, build_translation_font_plan, contains_translation_font_code,
    jis_to_shift_jis,
};
use crate::font::rasterize_character;
use crate::josa::{KoreanParticle, PARTICLE_FORMS, PARTICLE_MARKERS, plan_translation_characters};

#[test]
fn translation_codes_cover_ten_rows_without_an_illegal_trail_byte() {
    assert_eq!(jis_to_shift_jis(0x7521).unwrap(), [0xeb, 0x40]);
    assert_eq!(jis_to_shift_jis(0x755f).unwrap(), [0xeb, 0x7e]);
    assert_eq!(jis_to_shift_jis(0x7560).unwrap(), [0xeb, 0x80]);
    assert_eq!(jis_to_shift_jis(0x7e7e).unwrap(), [0xef, 0xfc]);
}

#[test]
#[ignore = "requires the Galmuri14, NeoDunggeunmo and BMJUA font files under assets/fonts (or MADOU456_ASSET_DIR/fonts)"]
fn translation_font_is_deterministic_and_keeps_unused_slots_blank() {
    let demand = ['한', '글', '가'].into_iter().collect::<BTreeSet<_>>();
    let source_codes = BTreeSet::from([[0xeb, 0x40]]);
    let first = build_translation_font_plan(&demand, &source_codes).unwrap();
    let second = build_translation_font_plan(&demand, &source_codes).unwrap();

    assert_eq!(first.codebook(), second.codebook());
    assert_eq!(first.sheet(), second.sheet());
    assert_eq!(first.occupancy(), second.occupancy());
    assert!(!first.codebook().values().any(|code| *code == [0xeb, 0x40]));
    assert_eq!(first.occupancy()[0], 0);
    assert_eq!(
        first.report().glyph_count,
        plan_translation_characters(&demand).unwrap().len()
    );
    assert_eq!(first.report().glyph_capacity, TRANSLATION_GLYPH_CAPACITY);
    assert!(
        first
            .sheet()
            .as_chunks::<{ crate::font::GLYPH_BYTES }>()
            .0
            .iter()
            .zip(first.occupancy())
            .all(|(glyph, occupied)| *occupied != 0 || glyph.iter().all(|byte| *byte == 0))
    );
    let code = first.codebook()[&'가'];
    let slot = translation_slot(code);
    let bitmap = rasterize_character('가').unwrap();
    assert_eq!(
        &first.sheet()[slot * 32..slot * 32 + 32],
        bitmap.as_slice(),
        "the resident glyph service must preserve each 16-pixel row as left byte then right byte"
    );
}

#[test]
#[ignore = "requires the Galmuri14, NeoDunggeunmo and BMJUA font files under assets/fonts (or MADOU456_ASSET_DIR/fonts)"]
fn translation_font_groups_hangul_and_reserves_runtime_particle_glyphs() {
    let mut demand = ['·', '가', '각', '나', '난']
        .into_iter()
        .collect::<BTreeSet<_>>();
    demand.insert(KoreanParticle::Subject.marker());
    let plan = build_translation_font_plan(&demand, &BTreeSet::from([[0xeb, 0x40]])).unwrap();

    for character in demand
        .iter()
        .chain(PARTICLE_FORMS.iter())
        .chain(PARTICLE_MARKERS.iter())
    {
        assert!(plan.codebook().contains_key(character));
    }
    let selector = plan.particle_selector();
    let subject_forms = selector.forms(KoreanParticle::Subject);
    assert_eq!(
        subject_forms[0],
        translation_slot(plan.codebook()[&'가']) as u16
    );
    assert_eq!(
        subject_forms[1],
        translation_slot(plan.codebook()[&'이']) as u16
    );
    assert!(translation_slot(plan.codebook()[&'·']) < usize::from(selector.first_hangul_slot()));
    assert!(
        translation_slot(plan.codebook()[&'가']) < usize::from(selector.first_with_batchim_slot())
    );
    assert!(
        translation_slot(plan.codebook()[&'각']) >= usize::from(selector.first_with_batchim_slot())
    );
    for marker in PARTICLE_MARKERS {
        let slot = translation_slot(plan.codebook()[&marker]);
        assert!(
            plan.sheet()[slot * 32..slot * 32 + 32]
                .iter()
                .all(|byte| *byte == 0)
        );
        assert_eq!(plan.occupancy()[slot], 1);
    }
}

fn translation_slot([lead, trail]: [u8; 2]) -> usize {
    let lead_base = usize::from(lead - 0xeb) * 188;
    if trail < 0x9f {
        lead_base + usize::from(trail - 0x40) - usize::from(trail >= 0x80)
    } else {
        lead_base + 94 + usize::from(trail - 0x9f)
    }
}

#[test]
fn translation_font_rejects_external_demand_missing_from_the_embedded_font() {
    let demand = ['한', '😀'].into_iter().collect::<BTreeSet<_>>();
    assert!(build_translation_font_plan(&demand, &BTreeSet::new()).is_err());
}

#[test]
fn translation_code_detection_requires_valid_pairs_in_reserved_rows() {
    assert!(contains_translation_font_code(&[0xeb, 0x40]));
    assert!(contains_translation_font_code(&[b'A', 0xef, 0xfc, b'B']));
    assert!(!contains_translation_font_code(&[0xea, 0xfc]));
    assert!(!contains_translation_font_code(&[0xeb, 0x7f]));
}
