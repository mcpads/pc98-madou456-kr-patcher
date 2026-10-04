use super::*;

#[test]
fn decodes_literals_and_overlapping_back_references() {
    let decoded = decode_stream(&[3, b'a', b'b', b'c', 0x83, 2, 0]).unwrap();
    assert_eq!(decoded.bytes, b"abcabcabc");
    assert_eq!(decoded.bytes_consumed, 7);
}

#[test]
fn decodes_zero_fill_before_the_output_start() {
    let decoded = decode_stream(&[0x80, 2, 0]).unwrap();
    assert_eq!(decoded.bytes, [0, 0, 0]);
}

#[test]
fn reports_only_whole_nonempty_stream_sequences() {
    let exact = decode_exact_compile_lz(&[1, b'a', 0, 2, b'b', b'c', 0]).unwrap();
    assert_eq!(exact.report.decoded_size, 3);
    assert_eq!(exact.report.streams.len(), 2);
    assert_eq!(exact.report.streams[1].input_offset, 3);
    assert!(decode_exact_compile_lz(&[1, b'a', 0, 0]).is_none());
    assert!(decode_exact_compile_lz(&[1, b'a']).is_none());
}

#[test]
fn encoded_stream_restores_repeated_and_literal_input() {
    let mut original = b"opening graphic ".repeat(40);
    original.extend(0_u8..=255);
    let packed = encode_compile_lz(&original);
    let decoded = decode_exact_compile_lz(&packed).unwrap();
    assert_eq!(decoded.streams, [original]);
}

#[test]
fn equal_length_match_uses_the_farthest_source() {
    let packed = encode_compile_lz(b"abcXabcYabcZ");
    assert_eq!(
        packed,
        [
            4, b'a', b'b', b'c', b'X', 0x80, 3, 1, b'Y', 0x80, 7, 1, b'Z', 0,
        ]
    );
}
