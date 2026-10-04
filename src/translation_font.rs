use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result, ensure};
use serde::Serialize;

use crate::font::{FontReport, GLYPH_BYTES, font_report, rasterize_character};
use crate::josa::{
    KoreanParticle, PARTICLE_MARKERS, is_particle_marker, plan_translation_characters,
};

pub(super) const TRANSLATION_GLYPH_CAPACITY: usize = 940;
const TRANSLATION_JIS_FIRST_ROW: u8 = 0x75;
const TRANSLATION_JIS_LAST_ROW: u8 = 0x7e;
const JIS_FIRST_CELL: u8 = 0x21;
const JIS_LAST_CELL: u8 = 0x7e;
const CELLS_PER_ROW: usize = 94;

#[derive(Debug, Clone, Eq, PartialEq, Serialize)]
pub(super) struct TranslationFontReport {
    pub(super) glyph_count: usize,
    pub(super) glyph_capacity: usize,
    pub(super) sheet_size: usize,
    pub(super) font: FontReport,
}

pub(super) struct TranslationFontPlan {
    codebook: BTreeMap<char, [u8; 2]>,
    occupancy: Vec<u8>,
    sheet: Vec<u8>,
    particle_selector: TranslationParticleSelector,
    report: TranslationFontReport,
}

impl TranslationFontPlan {
    pub(super) fn codebook(&self) -> &BTreeMap<char, [u8; 2]> {
        &self.codebook
    }

    pub(super) fn sheet(&self) -> &[u8] {
        &self.sheet
    }

    pub(super) fn occupancy(&self) -> &[u8] {
        &self.occupancy
    }

    pub(super) fn particle_selector(&self) -> &TranslationParticleSelector {
        &self.particle_selector
    }

    pub(super) fn report(&self) -> &TranslationFontReport {
        &self.report
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub(super) struct TranslationParticleSelector {
    first_hangul_slot: u16,
    first_with_batchim_slot: u16,
    marker_slots: [u16; 4],
    form_slots: [[u16; 2]; 4],
}

impl TranslationParticleSelector {
    fn from_plan(slot_by_character: &BTreeMap<char, u16>) -> Result<Self> {
        let first_hangul_slot = slot_by_character
            .iter()
            .filter(|(character, _)| crate::josa::is_modern_hangul(**character))
            .map(|(_, slot)| *slot)
            .min()
            .context("translation font plan has no Hangul glyph")?;
        let first_with_batchim_slot = slot_by_character
            .iter()
            .filter(|(character, _)| {
                crate::josa::is_modern_hangul(**character)
                    && crate::josa::has_batchim(**character).expect("planned glyph is Hangul")
            })
            .map(|(_, slot)| *slot)
            .min()
            .context("translation font plan has no batchim-bearing glyph")?;
        let marker_slots: [u16; 4] = PARTICLE_MARKERS
            .map(|marker| {
                slot_by_character.get(&marker).copied().with_context(|| {
                    format!("translation font lacks marker U+{:04X}", marker as u32)
                })
            })
            .into_iter()
            .collect::<Result<Vec<_>>>()?
            .try_into()
            .expect("particle marker count is fixed");
        let form_slots: [[u16; 2]; 4] = KoreanParticle::ALL
            .map(|particle| {
                let (without_batchim, with_batchim) = particle.forms();
                Ok([
                    *slot_by_character.get(&without_batchim).with_context(|| {
                        format!("translation font lacks particle form {without_batchim:?}")
                    })?,
                    *slot_by_character.get(&with_batchim).with_context(|| {
                        format!("translation font lacks particle form {with_batchim:?}")
                    })?,
                ])
            })
            .into_iter()
            .collect::<Result<Vec<_>>>()?
            .try_into()
            .expect("particle pair count is fixed");
        ensure!(
            marker_slots.windows(2).all(|slots| slots[0] < slots[1])
                && first_hangul_slot < first_with_batchim_slot
                && first_with_batchim_slot < marker_slots[0],
            "translation particle slot ranges overlap or are out of order"
        );
        Ok(Self {
            first_hangul_slot,
            first_with_batchim_slot,
            marker_slots,
            form_slots,
        })
    }

    pub(super) const fn first_hangul_slot(&self) -> u16 {
        self.first_hangul_slot
    }

    pub(super) const fn first_with_batchim_slot(&self) -> u16 {
        self.first_with_batchim_slot
    }

    pub(super) const fn marker_slots(&self) -> [u16; 4] {
        self.marker_slots
    }

    pub(super) const fn forms(&self, particle: KoreanParticle) -> [u16; 2] {
        self.form_slots[particle.index()]
    }
}

pub(super) fn build_translation_font_plan(
    characters: &BTreeSet<char>,
    source_codes: &BTreeSet<[u8; 2]>,
) -> Result<TranslationFontPlan> {
    ensure!(!characters.is_empty(), "translation font demand is empty");
    let planned_characters = plan_translation_characters(characters)?;
    ensure!(
        planned_characters.len() + source_codes.len() <= TRANSLATION_GLYPH_CAPACITY,
        "translation needs {} external glyphs and preserves {} source codes in a {TRANSLATION_GLYPH_CAPACITY}-cell renderer sheet",
        planned_characters.len(),
        source_codes.len()
    );
    ensure!(
        usize::from(TRANSLATION_JIS_LAST_ROW - TRANSLATION_JIS_FIRST_ROW + 1) * CELLS_PER_ROW
            == TRANSLATION_GLYPH_CAPACITY,
        "translation font geometry is inconsistent"
    );

    let mut codebook = BTreeMap::new();
    let mut slot_by_character = BTreeMap::new();
    let mut occupancy = vec![0_u8; TRANSLATION_GLYPH_CAPACITY];
    let mut sheet = vec![0_u8; TRANSLATION_GLYPH_CAPACITY * GLYPH_BYTES];
    let mut available_slots = (0..TRANSLATION_GLYPH_CAPACITY).filter(|slot| {
        let row = TRANSLATION_JIS_FIRST_ROW + (*slot / CELLS_PER_ROW) as u8;
        let cell = JIS_FIRST_CELL + (*slot % CELLS_PER_ROW) as u8;
        let code = jis_to_shift_jis(u16::from_be_bytes([row, cell]))
            .expect("translation sheet geometry creates valid codes");
        !source_codes.contains(&code)
    });
    for character in planned_characters.iter().copied() {
        ensure!(
            (!character.is_control() && !character.is_whitespace())
                || is_particle_marker(character),
            "external glyph demand contains unsupported character {character:?}"
        );
        let slot = available_slots
            .next()
            .context("translation font exhausted collision-free slots")?;
        let row = TRANSLATION_JIS_FIRST_ROW + u8::try_from(slot / CELLS_PER_ROW)?;
        let cell = JIS_FIRST_CELL + u8::try_from(slot % CELLS_PER_ROW)?;
        let encoded = jis_to_shift_jis(u16::from_be_bytes([row, cell]))?;
        let bitmap = if is_particle_marker(character) {
            [0_u8; GLYPH_BYTES]
        } else {
            rasterize_character(character)
                .with_context(|| format!("rasterize translation glyph {character:?}"))?
        };
        let start = slot * GLYPH_BYTES;
        sheet[start..start + GLYPH_BYTES].copy_from_slice(&bitmap);
        occupancy[slot] = 1;
        ensure!(
            codebook.insert(character, encoded).is_none(),
            "translation font repeats character {character:?}"
        );
        ensure!(
            slot_by_character
                .insert(character, u16::try_from(slot)?)
                .is_none(),
            "translation font repeats slot ownership for {character:?}"
        );
    }
    let particle_selector = TranslationParticleSelector::from_plan(&slot_by_character)?;
    let report = TranslationFontReport {
        glyph_count: codebook.len(),
        glyph_capacity: TRANSLATION_GLYPH_CAPACITY,
        sheet_size: sheet.len(),
        font: font_report()?,
    };
    Ok(TranslationFontPlan {
        codebook,
        occupancy,
        sheet,
        particle_selector,
        report,
    })
}

pub(super) fn jis_to_shift_jis(jis_code: u16) -> Result<[u8; 2]> {
    let [row, cell] = jis_code.to_be_bytes();
    ensure!(
        (TRANSLATION_JIS_FIRST_ROW..=TRANSLATION_JIS_LAST_ROW).contains(&row)
            && (JIS_FIRST_CELL..=JIS_LAST_CELL).contains(&cell),
        "JIS code 0x{jis_code:04X} is outside the translation font sheet"
    );
    let mut lead = ((row - 0x21) >> 1) + 0x81;
    if lead > 0x9f {
        lead += 0x40;
    }
    let trail = if row & 1 == 0 {
        cell + 0x7e
    } else {
        cell + 0x1f + u8::from(cell >= 0x60)
    };
    ensure!(
        trail != 0x7f,
        "translation font generated an illegal Shift-JIS trail byte"
    );
    Ok([lead, trail])
}

pub(super) fn contains_translation_font_code(bytes: &[u8]) -> bool {
    bytes
        .windows(2)
        .any(|pair| is_translation_font_code([pair[0], pair[1]]))
}

pub(super) fn is_translation_font_code([lead, trail]: [u8; 2]) -> bool {
    (0xeb..=0xef).contains(&lead)
        && ((0x40..=0x7e).contains(&trail) || (0x80..=0xfc).contains(&trail))
}

#[cfg(test)]
#[path = "translation_font_tests.rs"]
mod tests;
