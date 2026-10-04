use super::find_unique_subslice;

#[test]
fn finds_only_unique_byte_sequences() {
    assert_eq!(find_unique_subslice(b"abc", b"b"), Some(1));
    assert_eq!(find_unique_subslice(b"aba", b"a"), None);
    assert_eq!(find_unique_subslice(b"abc", b"z"), None);
}
