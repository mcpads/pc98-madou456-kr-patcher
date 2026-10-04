use super::encode_short_name;

#[test]
fn encodes_supported_game_style_names() {
    assert_eq!(encode_short_name("MADO456.COM").unwrap(), *b"MADO456 COM");
    assert_eq!(encode_short_name("MSG.DAT").unwrap(), *b"MSG     DAT");
}

#[test]
fn rejects_names_that_cannot_be_inserted_without_aliasing() {
    assert!(encode_short_name("TOO-LONG-NAME.DAT").is_err());
    assert!(encode_short_name("A.B.C").is_err());
    assert!(encode_short_name("한글.DAT").is_err());
}
