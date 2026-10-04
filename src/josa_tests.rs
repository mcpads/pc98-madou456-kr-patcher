use super::*;

#[test]
fn splits_only_nonempty_manually_authored_message_lines() {
    assert_eq!(
        manual_message_lines("첫 줄\n둘째 줄").unwrap(),
        ["첫 줄", "둘째 줄"]
    );
    assert!(manual_message_lines("첫 줄\n").is_err());
    assert!(manual_message_lines("\n둘째 줄").is_err());
    assert!(manual_message_lines("첫 줄\n\n셋째 줄").is_err());
}

#[test]
fn subject_particle_follows_the_preceding_hangul_batchim() {
    assert_eq!(KoreanParticle::Subject.select_for('주').unwrap(), '가');
    assert_eq!(KoreanParticle::Subject.select_for('풀').unwrap(), '이');
}

#[test]
fn runtime_particle_syntax_is_one_renderer_cell() {
    let characters = runtime_text_characters("품목{josa:이} 들어 있다").unwrap();

    assert_eq!(characters.len(), 9);
    assert_eq!(characters[2], KoreanParticle::Subject.marker());
    assert!(!characters.contains(&'{'));
}

#[test]
fn malformed_or_raw_runtime_particle_markers_are_rejected() {
    assert!(runtime_text_characters("{josa:이").is_err());
    assert!(runtime_text_characters("{josa:으로}").is_err());
    assert!(runtime_text_characters(&KoreanParticle::Subject.marker().to_string()).is_err());
}
