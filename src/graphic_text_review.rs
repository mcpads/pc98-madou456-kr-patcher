use std::collections::BTreeSet;

use anyhow::{Result, ensure};
use serde::Serialize;

const TEXT_CANDIDATES: [GraphicTextCandidate; 8] = [
    GraphicTextCandidate {
        name: "CFG.DAT",
        observed_role: "configuration menu glyphs",
        consumer_status: "program-linked",
    },
    GraphicTextCandidate {
        name: "CFG_N.DAT",
        observed_role: "configuration menu alternate-state glyphs",
        consumer_status: "program-linked",
    },
    GraphicTextCandidate {
        name: "CFG_S.DAT",
        observed_role: "configuration menu variant glyphs",
        consumer_status: "unresolved",
    },
    GraphicTextCandidate {
        name: "C_CHAR1.DAT",
        observed_role: "team-selection prompt and player labels",
        consumer_status: "program-linked",
    },
    GraphicTextCandidate {
        name: "FIN.DAT",
        observed_role: "ending title and credits",
        consumer_status: "program-linked",
    },
    GraphicTextCandidate {
        name: "MAP_CHR.DAT",
        observed_role: "gameplay HUD and location labels",
        consumer_status: "program-linked",
    },
    GraphicTextCandidate {
        name: "SELECT.DAT",
        observed_role: "team-selection prompt and player labels",
        consumer_status: "program-linked",
    },
    GraphicTextCandidate {
        name: "TITLE.DAT",
        observed_role: "title menu labels",
        consumer_status: "program-linked",
    },
];

const NO_VISIBLE_GAME_TEXT: [&str; 30] = [
    "BEAM.DAT",
    "BG_S.DAT",
    "CURKUN.DAT",
    "C_CHAR2.DAT",
    "EC_1_S.DAT",
    "EC_2_S.DAT",
    "EC_3_S.DAT",
    "EC_4_S.DAT",
    "EC_5_S.DAT",
    "EC_6_S.DAT",
    "ED1_1.DAT",
    "ED1_2.DAT",
    "ED_2.DAT",
    "ED_3.DAT",
    "ED_4.DAT",
    "ED_5.DAT",
    "ED_6.DAT",
    "FACE.DAT",
    "ICON.DAT",
    "MM_CK.DAT",
    "MM_DK.DAT",
    "MM_FDA.DAT",
    "MM_FDB.DAT",
    "MM_SR1A.DAT",
    "MM_SR1B.DAT",
    "MM_SR2A.DAT",
    "MM_SR2B.DAT",
    "MM_TWA.DAT",
    "MM_TWB.DAT",
    "OP_PG_S.DAT",
];

#[derive(Clone, Copy, Debug, Serialize)]
pub(super) struct GraphicTextCandidate {
    pub(super) name: &'static str,
    observed_role: &'static str,
    pub(super) consumer_status: &'static str,
}

#[derive(Debug, Serialize)]
pub(super) struct GraphicTextReview {
    review_basis: &'static str,
    scope: &'static str,
    limitation: &'static str,
    text_candidate_count: usize,
    text_candidates: Vec<GraphicTextCandidate>,
    no_visible_game_text_count: usize,
    no_visible_game_text_atlases: Vec<&'static str>,
}

pub(super) fn review_graphic_text_surfaces(
    consumer_confirmed: &BTreeSet<&str>,
    consumer_unresolved: &BTreeSet<&str>,
) -> Result<GraphicTextReview> {
    let all_atlases: BTreeSet<&str> = consumer_confirmed
        .union(consumer_unresolved)
        .copied()
        .collect();
    let text_candidate_names: BTreeSet<&str> = TEXT_CANDIDATES
        .iter()
        .map(|candidate| candidate.name)
        .collect();
    let no_visible_names: BTreeSet<&str> = NO_VISIBLE_GAME_TEXT.into_iter().collect();
    ensure!(
        text_candidate_names.is_disjoint(&no_visible_names),
        "graphic text review classifications overlap"
    );
    let reviewed_names: BTreeSet<&str> = text_candidate_names
        .union(&no_visible_names)
        .copied()
        .collect();
    ensure!(
        reviewed_names == all_atlases,
        "graphic text review does not cover the verified masked-tile atlas population"
    );
    for candidate in TEXT_CANDIDATES {
        let expected_status = if consumer_confirmed.contains(candidate.name) {
            "program-linked"
        } else if consumer_unresolved.contains(candidate.name) {
            "unresolved"
        } else {
            "missing"
        };
        ensure!(
            candidate.consumer_status == expected_status,
            "graphic text candidate {} consumer status changed",
            candidate.name
        );
    }

    Ok(GraphicTextReview {
        review_basis: "private diagnostic renders of the decoded 16x16 mask plane and four color-bit planes; diagnostic colors are not the runtime palette",
        scope: "visible game UI, HUD, location, ending, and credit text; atlas tile-index or developer markers are excluded",
        limitation: "a visible-text candidate identifies pixels requiring translation review, not runtime reachability or a finalized translation target",
        text_candidate_count: TEXT_CANDIDATES.len(),
        text_candidates: TEXT_CANDIDATES.to_vec(),
        no_visible_game_text_count: NO_VISIBLE_GAME_TEXT.len(),
        no_visible_game_text_atlases: NO_VISIBLE_GAME_TEXT.to_vec(),
    })
}

pub(super) fn translation_graphic_candidates() -> Vec<GraphicTextCandidate> {
    TEXT_CANDIDATES.to_vec()
}

#[cfg(test)]
#[path = "graphic_text_review_tests.rs"]
mod tests;
