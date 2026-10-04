use super::{ascii_case_insensitive_match_count, parse_padded_filename};

#[test]
fn parses_space_padded_fixed_table_filenames() {
    assert_eq!(
        parse_padded_filename(b"MM_FDA  .DAT\0", 0).unwrap(),
        "MM_FDA.DAT"
    );
    assert!(parse_padded_filename(b"MM_FDA  -DAT\0", 0).is_err());
}

#[test]
fn counts_case_insensitive_filename_literals_without_partial_matches() {
    assert_eq!(
        ascii_case_insensitive_match_count(b"$cfg_s.dat\0CFG_S.DAT", b"CFG_S.DAT"),
        2
    );
    assert_eq!(
        ascii_case_insensitive_match_count(b"CFG_S   .DAT", b"CFG_S.DAT"),
        0
    );
    assert_eq!(ascii_case_insensitive_match_count(b"anything", b""), 0);
}
