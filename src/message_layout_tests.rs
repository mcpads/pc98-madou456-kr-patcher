use super::{
    COMMON_QUOTED_DIALOGUE, CONFIRMED_OPENING_MESSAGE_PANE, MERCHANT_DIALOGUE, MessagePaneProfile,
    QUEST_DESCRIPTION, STANDARD_DIALOGUE_MESSAGE_FAMILY, detect_message_overflows, message_profile,
};
use crate::localization::message_analysis::MessageToken;

const TEST_PANE: MessagePaneProfile = MessagePaneProfile {
    id: "test_pane",
    first_text_column: 1,
    last_text_column: 23,
    first_text_row: 1,
    last_text_row: 4,
    evidence_level: "test_evidence",
    geometry_basis: "test geometry",
};

fn column(value: &str) -> MessageToken {
    MessageToken::Control {
        opcode_hex: "00".to_owned(),
        argument_hex: Some(value.to_owned()),
    }
}

fn row(value: &str) -> MessageToken {
    MessageToken::Control {
        opcode_hex: "01".to_owned(),
        argument_hex: Some(value.to_owned()),
    }
}

fn text() -> MessageToken {
    source_text("")
}

fn source_text(value: &str) -> MessageToken {
    MessageToken::Text {
        source_hex: String::new(),
        cp932_hex: String::new(),
        text: value.to_owned(),
    }
}

#[test]
fn groups_adjacent_event_character_and_quoted_common_dialogue_candidates() {
    assert_eq!(
        message_profile("MSGEV.DAT:22", &[source_text("개막")])
            .unwrap()
            .unwrap()
            .id,
        CONFIRMED_OPENING_MESSAGE_PANE.id
    );
    assert_eq!(
        message_profile("MSGEV.DAT:21", &[source_text("목적지")])
            .unwrap()
            .unwrap()
            .id,
        STANDARD_DIALOGUE_MESSAGE_FAMILY.id
    );
    assert_eq!(
        message_profile("MSG07.DAT:C0", &[source_text("반응")])
            .unwrap()
            .unwrap()
            .id,
        STANDARD_DIALOGUE_MESSAGE_FAMILY.id
    );
    assert_eq!(
        message_profile("MSG.DAT:A0", &[source_text("「안내")])
            .unwrap()
            .unwrap()
            .id,
        COMMON_QUOTED_DIALOGUE.id
    );
    assert_eq!(
        message_profile("MSG.DAT:C8", &[source_text("「상점")])
            .unwrap()
            .unwrap()
            .id,
        MERCHANT_DIALOGUE.id
    );
    assert_eq!(
        message_profile("MSG.DAT:F0", &[source_text("「목표")])
            .unwrap()
            .unwrap()
            .id,
        QUEST_DESCRIPTION.id
    );
    assert!(
        message_profile("MSG.DAT:58", &[source_text("메뉴")])
            .unwrap()
            .is_none()
    );
}

fn next_line() -> MessageToken {
    MessageToken::Control {
        opcode_hex: "02".to_owned(),
        argument_hex: None,
    }
}

#[test]
fn reports_cells_past_the_proven_right_edge() {
    let translated = vec!["가".repeat(25)];
    let findings = detect_message_overflows(
        "MSGEV.DAT:22",
        &[column("01"), row("01"), text()],
        &translated,
        TEST_PANE,
    )
    .unwrap();
    let overflows = findings.horizontal;

    assert_eq!(overflows.len(), 1);
    assert_eq!(overflows[0].start_column, 1);
    assert_eq!(overflows[0].cell_count, 25);
    assert_eq!(overflows[0].source_cell_count, 0);
    assert_eq!(overflows[0].available_cell_count, 23);
    assert_eq!(overflows[0].overflow_cell_count, 2);
    assert_eq!(translated, vec!["가".repeat(25)]);
}

#[test]
fn accepts_a_segment_that_exactly_fills_the_proven_width() {
    let translated = vec!["가".repeat(23)];

    assert!(
        detect_message_overflows(
            "fixture",
            &[column("01"), row("01"), text()],
            &translated,
            TEST_PANE,
        )
        .unwrap()
        .horizontal
        .is_empty()
    );
}

#[test]
fn honors_an_explicit_segment_start_column() {
    let translated = vec!["가".repeat(12)];
    let overflows = detect_message_overflows(
        "fixture",
        &[column("0d"), row("01"), text()],
        &translated,
        TEST_PANE,
    )
    .unwrap()
    .horizontal;

    assert_eq!(overflows[0].start_column, 13);
    assert_eq!(overflows[0].available_cell_count, 11);
    assert_eq!(overflows[0].overflow_cell_count, 1);
}

#[test]
fn resets_to_the_authored_base_column_after_a_line_control() {
    let translated = vec!["가".repeat(23), "나".repeat(23)];

    assert!(
        detect_message_overflows(
            "fixture",
            &[column("01"), row("01"), text(), next_line(), text(),],
            &translated,
            TEST_PANE,
        )
        .unwrap()
        .horizontal
        .is_empty()
    );
}

#[test]
fn rejects_a_candidate_profile_that_does_not_contain_its_source_text() {
    let translated = vec!["번역".to_owned()];
    let error = detect_message_overflows(
        "fixture",
        &[column("01"), row("01"), source_text(&"원".repeat(24))],
        &translated,
        TEST_PANE,
    )
    .unwrap_err();

    assert!(error.to_string().contains("does not fit profile"));
}

#[test]
fn reports_only_manually_authored_lines_past_the_profile_height() {
    let translated = vec!["하나\n둘\n셋\n넷\n다섯".to_owned()];
    let findings = detect_message_overflows(
        "fixture",
        &[column("01"), row("01"), source_text("원문")],
        &translated,
        TEST_PANE,
    )
    .unwrap();

    assert!(findings.horizontal.is_empty());
    assert_eq!(findings.vertical.len(), 1);
    assert_eq!(findings.vertical[0].manual_line_index, 4);
    assert_eq!(findings.vertical[0].text_row, 5);
    assert_eq!(findings.vertical[0].overflow_row_count, 1);
}
