use super::{FixedRangeExpectedWrite, apply_fixed_range_expected_writes};

fn write(
    owner: &'static str,
    offset: usize,
    expected_source: &[u8],
    replacement: &[u8],
) -> FixedRangeExpectedWrite {
    FixedRangeExpectedWrite {
        owner,
        purpose: "fixture replacement",
        offset,
        expected_source: expected_source.to_vec(),
        replacement: replacement.to_vec(),
    }
}

#[test]
fn fixed_range_writes_verify_the_immutable_source_before_application() {
    let source = b"ABCDEFGH";
    let writes = [
        write("second", 6, b"GH", b"78"),
        write("first", 1, b"BC", b"23"),
    ];

    let output = apply_fixed_range_expected_writes(source, &writes).unwrap();

    assert_eq!(&output, b"A23DEF78");
    assert_eq!(source, b"ABCDEFGH");
}

#[test]
fn source_mismatch_rejects_the_whole_write_plan() {
    let source = b"ABCDEFGH";
    let writes = [
        write("first", 1, b"BC", b"23"),
        write("mismatch", 5, b"XX", b"67"),
    ];

    let error = apply_fixed_range_expected_writes(source, &writes).unwrap_err();

    assert!(error.to_string().contains("immutable source"));
    assert_eq!(source, b"ABCDEFGH");
}

#[test]
fn overlapping_writers_are_rejected_before_application() {
    let source = b"ABCDEFGH";
    let writes = [
        write("first", 1, b"BCD", b"234"),
        write("second", 3, b"DE", b"45"),
    ];

    let error = apply_fixed_range_expected_writes(source, &writes).unwrap_err();

    assert!(error.to_string().contains("overlap"));
    assert_eq!(source, b"ABCDEFGH");
}
