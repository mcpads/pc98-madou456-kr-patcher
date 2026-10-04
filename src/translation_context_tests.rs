use super::{
    ContextSelector, SourceCatalogBinding, TranslationContext, TranslationContextCatalog,
    audit_context_catalog, parse_selector_ranges,
};

const MESSAGE_HASH: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const INLINE_HASH: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

#[test]
fn assigns_sparse_source_entries_without_treating_range_gaps_as_entries() {
    let catalog = fixture();
    let assignments = audit_context_catalog(
        &catalog,
        MESSAGE_HASH,
        &["MSG.DAT:00".to_owned(), "MSG.DAT:02".to_owned()],
        INLINE_HASH,
        &["opening_narration:000".to_owned()],
    )
    .unwrap();

    assert_eq!(assignments.len(), 3);
    assert_eq!(assignments["MSG.DAT:02"], "message_context");
    assert_eq!(assignments["opening_narration:000"], "inline_context");
}

#[test]
fn rejects_overlap_and_missing_source_entries() {
    let mut catalog = fixture();
    catalog.contexts[1].selectors.push(ContextSelector {
        collection: "messages".to_owned(),
        resource: "MSG.DAT".to_owned(),
        ranges: vec!["02".to_owned()],
    });
    assert!(
        audit_context_catalog(
            &catalog,
            MESSAGE_HASH,
            &["MSG.DAT:00".to_owned(), "MSG.DAT:02".to_owned()],
            INLINE_HASH,
            &["opening_narration:000".to_owned()],
        )
        .is_err()
    );

    let catalog = fixture();
    assert!(
        audit_context_catalog(
            &catalog,
            MESSAGE_HASH,
            &[
                "MSG.DAT:00".to_owned(),
                "MSG.DAT:02".to_owned(),
                "MSG.DAT:03".to_owned(),
            ],
            INLINE_HASH,
            &["opening_narration:000".to_owned()],
        )
        .is_err()
    );
}

#[test]
fn requires_canonical_ordered_selector_ranges() {
    for ranges in [
        vec!["0a".to_owned()],
        vec!["0A-09".to_owned()],
        vec!["00-02".to_owned(), "02-03".to_owned()],
    ] {
        let selector = ContextSelector {
            collection: "messages".to_owned(),
            resource: "MSG.DAT".to_owned(),
            ranges,
        };
        assert!(parse_selector_ranges(&selector).is_err());
    }
}

fn fixture() -> TranslationContextCatalog {
    TranslationContextCatalog {
        schema: "pc98_madou456.translation_contexts".to_owned(),
        source_catalogs: vec![
            SourceCatalogBinding {
                id: "messages".to_owned(),
                sha256: MESSAGE_HASH.to_owned(),
                entry_count: 2,
            },
            SourceCatalogBinding {
                id: "inline_text".to_owned(),
                sha256: INLINE_HASH.to_owned(),
                entry_count: 1,
            },
        ],
        contexts: vec![
            TranslationContext {
                id: "message_context".to_owned(),
                surface: "common_ui".to_owned(),
                label: "messages".to_owned(),
                speaker: None,
                context_basis: "content_structure_inferred".to_owned(),
                sample_requirement: "optional".to_owned(),
                review_focus: vec!["meaning".to_owned()],
                selectors: vec![ContextSelector {
                    collection: "messages".to_owned(),
                    resource: "MSG.DAT".to_owned(),
                    ranges: vec!["00-02".to_owned()],
                }],
            },
            TranslationContext {
                id: "inline_context".to_owned(),
                surface: "opening".to_owned(),
                label: "opening".to_owned(),
                speaker: None,
                context_basis: "consumer_role_confirmed".to_owned(),
                sample_requirement: "optional".to_owned(),
                review_focus: vec!["layout".to_owned()],
                selectors: vec![ContextSelector {
                    collection: "inline_text".to_owned(),
                    resource: "opening_narration".to_owned(),
                    ranges: vec!["000".to_owned()],
                }],
            },
        ],
    }
}
