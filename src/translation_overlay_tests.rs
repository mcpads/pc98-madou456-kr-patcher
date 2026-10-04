use super::*;

#[test]
fn audits_translated_and_source_preserved_selected_entries() {
    let selection = selection_fixture();
    let overlay = overlay_fixture();
    let context_assignments = context_assignments();
    let message_source_cell_counts = BTreeMap::from([("MSG.DAT:00".to_owned(), vec![1])]);
    let inline_text_source_cell_counts =
        BTreeMap::from([("staff_credits:003".to_owned(), vec![4])]);
    let source_context = OverlaySourceContext {
        contexts_sha256: "context-sha",
        context_assignments: &context_assignments,
        message_catalog_sha256: "message-sha",
        message_source_cell_counts: &message_source_cell_counts,
        inline_text_catalog_sha256: "inline-sha",
        inline_text_source_cell_counts: &inline_text_source_cell_counts,
    };
    let audit = audit_overlay(&selection, "selection-sha", &overlay, &source_context).unwrap();

    assert_eq!(audit.translated_entry_count, 1);
    assert_eq!(audit.preserve_source_entry_count, 1);
    assert_eq!(
        audit.required_external_characters,
        BTreeSet::from(['글', '한'])
    );
    assert_eq!(audit.longer_than_source_segments, ["MSG.DAT:00#0"]);

    let mut draft = super::super::translation_draft::build_translation_draft(
        "message-sha",
        [("MSG.DAT:00", 1)],
        "inline-sha",
        [("staff_credits:003", 1)],
    )
    .unwrap();
    for entry in &overlay.entries {
        draft
            .apply_translation(
                &entry.collection,
                &entry.id,
                &entry.ko_segments,
                &entry.status,
                &entry.notes,
            )
            .unwrap();
    }
    assert_eq!(
        draft.collection_segments(MESSAGE_COLLECTION).unwrap()["MSG.DAT:00"],
        ["한글"]
    );
}

#[test]
fn rejects_changed_selection_segment_population_and_japanese_script() {
    let selection = selection_fixture();
    let mut overlay = overlay_fixture();
    overlay.entries.swap(0, 1);
    assert!(
        audit_fixture(&selection, &overlay).is_err(),
        "selection order must be stable"
    );

    let mut overlay = overlay_fixture();
    overlay.entries[0].ko_segments.push("덧붙임".to_owned());
    assert!(audit_fixture(&selection, &overlay).is_err());

    let mut overlay = overlay_fixture();
    overlay.entries[0].ko_segments[0] = "한글かな".to_owned();
    assert!(audit_fixture(&selection, &overlay).is_err());

    let mut selection = selection_fixture();
    selection.entries[0].context_id = "wrong_context".to_owned();
    assert!(audit_fixture(&selection, &overlay_fixture()).is_err());
}

#[test]
fn accepts_a_context_batch_selection_with_matching_scope_and_id() {
    let mut selection = selection_fixture();
    selection.schema = BATCH_SELECTION_SCHEMA.to_owned();
    selection.selection_status = None;
    selection.batch_id = Some("test_batch".to_owned());
    let mut overlay = overlay_fixture();
    overlay.scope = CONTEXT_BATCH_SCOPE.to_owned();
    overlay.selection.id = "test_batch".to_owned();

    assert!(audit_fixture(&selection, &overlay).is_ok());
}

#[test]
fn particle_syntax_counts_as_one_cell_and_demands_only_its_marker() {
    let selection = selection_fixture();
    let mut overlay = overlay_fixture();
    overlay.entries[0].ko_segments[0] = "한{josa:이}".to_owned();

    let audit = audit_fixture(&selection, &overlay).unwrap();
    let marker = crate::josa::KoreanParticle::Subject.marker();

    assert_eq!(audit.maximum_segment_cell_count, 2);
    assert!(audit.required_external_characters.contains(&'한'));
    assert!(audit.required_external_characters.contains(&marker));
    assert!(!audit.unique_non_whitespace_characters.contains(&'{'));
}

#[test]
fn rejects_entry_ownership_shared_by_two_batches() {
    let overlay = overlay_fixture();
    let mut seen_entry_ids = BTreeSet::new();

    record_entry_ownership(&mut seen_entry_ids, &overlay).unwrap();
    assert!(record_entry_ownership(&mut seen_entry_ids, &overlay).is_err());
}

fn audit_fixture(
    selection: &TranslationSelection,
    overlay: &TranslationOverlay,
) -> Result<OverlayAudit> {
    let context_assignments = context_assignments();
    let message_source_cell_counts = BTreeMap::from([("MSG.DAT:00".to_owned(), vec![1])]);
    let inline_text_source_cell_counts =
        BTreeMap::from([("staff_credits:003".to_owned(), vec![4])]);
    let source_context = OverlaySourceContext {
        contexts_sha256: "context-sha",
        context_assignments: &context_assignments,
        message_catalog_sha256: "message-sha",
        message_source_cell_counts: &message_source_cell_counts,
        inline_text_catalog_sha256: "inline-sha",
        inline_text_source_cell_counts: &inline_text_source_cell_counts,
    };
    audit_overlay(selection, "selection-sha", overlay, &source_context)
}

fn context_assignments() -> BTreeMap<String, String> {
    BTreeMap::from([
        ("MSG.DAT:00".to_owned(), "names".to_owned()),
        ("staff_credits:003".to_owned(), "staff_names".to_owned()),
    ])
}

fn selection_fixture() -> TranslationSelection {
    TranslationSelection {
        schema: REPRESENTATIVE_SELECTION_SCHEMA.to_owned(),
        context_catalog_sha256: "context-sha".to_owned(),
        selection_status: Some("selected_for_first_draft".to_owned()),
        batch_id: None,
        entries: vec![
            RepresentativeSelectionEntry {
                id: "MSG.DAT:00".to_owned(),
                context_id: "names".to_owned(),
                goals: vec!["terminology".to_owned()],
            },
            RepresentativeSelectionEntry {
                id: "staff_credits:003".to_owned(),
                context_id: "staff_names".to_owned(),
                goals: vec!["source_preservation".to_owned()],
            },
        ],
    }
}

fn overlay_fixture() -> TranslationOverlay {
    TranslationOverlay {
        schema: OVERLAY_SCHEMA.to_owned(),
        scope: REPRESENTATIVE_SCOPE.to_owned(),
        source_catalogs: vec![
            SourceCatalogBinding {
                id: MESSAGE_COLLECTION.to_owned(),
                sha256: "message-sha".to_owned(),
                entry_count: 1,
            },
            SourceCatalogBinding {
                id: INLINE_TEXT_COLLECTION.to_owned(),
                sha256: "inline-sha".to_owned(),
                entry_count: 1,
            },
        ],
        selection: SelectionBinding {
            id: "representative_sample".to_owned(),
            sha256: "selection-sha".to_owned(),
            entry_count: 2,
        },
        entries: vec![
            TranslationOverlayEntry {
                collection: MESSAGE_COLLECTION.to_owned(),
                id: "MSG.DAT:00".to_owned(),
                ko_segments: vec!["한글".to_owned()],
                status: "needs_human_review".to_owned(),
                notes: "test translation".to_owned(),
            },
            TranslationOverlayEntry {
                collection: INLINE_TEXT_COLLECTION.to_owned(),
                id: "staff_credits:003".to_owned(),
                ko_segments: vec![String::new()],
                status: PRESERVE_SOURCE_STATUS.to_owned(),
                notes: "personal name".to_owned(),
            },
        ],
    }
}
