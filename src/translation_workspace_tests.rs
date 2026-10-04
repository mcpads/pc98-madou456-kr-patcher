use super::translation_graphics;

#[test]
fn separates_program_linked_graphics_from_unresolved_review_material() {
    let graphics = translation_graphics().unwrap();
    assert_eq!(graphics.candidate_count, 8);
    assert_eq!(graphics.program_linked_count, 7);
    assert_eq!(graphics.unresolved_count, 1);

    let unresolved = graphics
        .candidates
        .iter()
        .filter(|candidate| candidate.consumer_status == "unresolved")
        .map(|candidate| candidate.name)
        .collect::<Vec<_>>();
    assert_eq!(unresolved, ["CFG_S.DAT"]);
}
