use super::{TranslationDraft, audit_translation_draft, build_translation_draft};

const MESSAGE_HASH: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const INLINE_HASH: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

#[test]
fn builds_a_source_free_segmented_translation_overlay() {
    let draft = fixture();
    let encoded = serde_json::to_string(&draft).unwrap();

    assert_eq!(draft.entry_count(), 3);
    assert!(encoded.contains("MSG.DAT:00"));
    assert!(!encoded.contains("source_hex"));
    assert!(!encoded.contains("decoded_text"));
    assert!(!encoded.contains("アルル"));
}

#[test]
fn rejects_changed_ids_and_inconsistent_translation_statuses() {
    let mut draft = fixture();
    draft.collections[0].entries[0].id = "MSG.DAT:01".to_owned();
    assert!(audit_fixture(&draft).is_err());

    let mut draft = fixture();
    draft.collections[0].entries[0].ko_segments[0] = "아르르".to_owned();
    assert!(audit_fixture(&draft).is_err());

    draft.collections[0].entries[0].status = "needs_human_review".to_owned();
    assert!(audit_fixture(&draft).is_ok());

    draft.collections[0].entries[0].ko_segments.pop();
    assert!(audit_fixture(&draft).is_err());
}

#[test]
fn accepts_each_wording_state_without_treating_it_as_a_release_gate() {
    for status in [
        "in_progress",
        "needs_review",
        "needs_human_review",
        "approved",
    ] {
        let mut draft = fixture();
        draft.collections[0].entries[0].ko_segments[0] = "아르르".to_owned();
        draft.collections[0].entries[0].status = status.to_owned();
        assert!(audit_fixture(&draft).is_ok(), "status {status}");
    }

    let mut draft = fixture();
    draft.collections[0].entries[0].status = "preserve_source".to_owned();
    draft.collections[0].entries[0].notes = "staff name".to_owned();
    assert!(audit_fixture(&draft).is_ok());
}

#[test]
fn requires_a_reason_for_source_preservation() {
    let mut draft = fixture();
    draft.collections[0].entries[0].status = "preserve_source".to_owned();
    assert!(audit_fixture(&draft).is_err());

    draft.collections[0].entries[0].notes = "personal credit".to_owned();
    draft.collections[0].entries[0].ko_segments[0] = "번역".to_owned();
    assert!(audit_fixture(&draft).is_err());
}

#[test]
fn rejects_control_characters_inside_translation_segments() {
    for control in ['\0', '\r'] {
        let mut draft = fixture();
        draft.collections[0].entries[0].ko_segments[0] = format!("아{control}르");
        draft.collections[0].entries[0].status = "needs_human_review".to_owned();
        assert!(audit_fixture(&draft).is_err());
    }

    let mut message_draft = fixture();
    message_draft.collections[0].entries[0].ko_segments[0] = "첫 줄\n둘째 줄".to_owned();
    message_draft.collections[0].entries[0].status = "needs_human_review".to_owned();
    assert!(audit_fixture(&message_draft).is_ok());

    let mut inline_draft = fixture();
    inline_draft.collections[1].entries[0].ko_segments[0] = "첫 줄\n둘째 줄".to_owned();
    inline_draft.collections[1].entries[0].status = "needs_human_review".to_owned();
    assert!(audit_fixture(&inline_draft).is_err());
}

#[test]
fn rejects_empty_manual_message_lines() {
    for text in ["\n둘째 줄", "첫 줄\n", "첫 줄\n\n셋째 줄"] {
        let mut draft = fixture();
        draft.collections[0].entries[0].ko_segments[0] = text.to_owned();
        draft.collections[0].entries[0].status = "needs_human_review".to_owned();
        assert!(audit_fixture(&draft).is_err());
    }
}

#[test]
fn rejects_malformed_runtime_particle_syntax() {
    for text in ["{josa:이", "{josa:으로}"] {
        let mut draft = fixture();
        draft.collections[0].entries[0].ko_segments[0] = text.to_owned();
        draft.collections[0].entries[0].status = "needs_human_review".to_owned();
        assert!(audit_fixture(&draft).is_err());
    }
}

#[test]
fn rejects_unknown_translation_draft_fields() {
    let encoded = serde_json::to_value(fixture()).unwrap();
    let mut object = encoded.as_object().unwrap().clone();
    object.insert(
        "source_text".to_owned(),
        serde_json::Value::String("x".to_owned()),
    );
    assert!(serde_json::from_value::<TranslationDraft>(object.into()).is_err());
}

fn fixture() -> TranslationDraft {
    build_translation_draft(
        MESSAGE_HASH,
        [("MSG.DAT:00", 2), ("MSG.DAT:02", 1)],
        INLINE_HASH,
        [("opening_narration:000", 1)],
    )
    .unwrap()
}

fn audit_fixture(draft: &TranslationDraft) -> anyhow::Result<()> {
    audit_translation_draft(
        draft,
        MESSAGE_HASH,
        [("MSG.DAT:00", 2), ("MSG.DAT:02", 1)],
        INLINE_HASH,
        [("opening_narration:000", 1)],
    )
}
