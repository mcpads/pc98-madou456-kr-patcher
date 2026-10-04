use std::collections::BTreeSet;

use super::{NO_VISIBLE_GAME_TEXT, TEXT_CANDIDATES, review_graphic_text_surfaces};

#[test]
fn requires_a_complete_non_overlapping_atlas_review() {
    let mut confirmed: BTreeSet<&str> = TEXT_CANDIDATES
        .iter()
        .map(|candidate| candidate.name)
        .chain(NO_VISIBLE_GAME_TEXT)
        .collect();
    confirmed.remove("CFG_S.DAT");
    let unresolved = BTreeSet::from(["CFG_S.DAT"]);
    let review = review_graphic_text_surfaces(&confirmed, &unresolved).unwrap();
    assert_eq!(review.text_candidate_count, 8);
    assert_eq!(review.no_visible_game_text_count, 30);

    confirmed.remove("TITLE.DAT");
    assert!(review_graphic_text_surfaces(&confirmed, &unresolved).is_err());
}
