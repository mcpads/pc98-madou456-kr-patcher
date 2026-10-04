use super::{make_directory_entry, short_name_text};

#[test]
fn round_trips_visible_short_name_fields() {
    let raw = make_directory_entry(*b"MADO456 COM", 0x20, 42, 1234).unwrap();
    assert_eq!(
        short_name_text(raw[..11].try_into().unwrap()),
        "MADO456.COM"
    );
    assert_eq!(u16::from_le_bytes(raw[26..28].try_into().unwrap()), 42);
    assert_eq!(u32::from_le_bytes(raw[28..32].try_into().unwrap()), 1234);
}
