use super::{
    CgPortAddress, EXTERNAL_CHARACTER_CANDIDATE_CAPACITY, candidate_addresses, drbios_cg_address,
    inline_cg_address,
};

#[test]
fn maps_normal_shift_jis_to_the_same_cg_address() {
    let expected = CgPortAddress { a1: 0x22, a3: 0x04 };
    assert_eq!(drbios_cg_address([0x82, 0xa0]).unwrap(), expected);
    assert_eq!(inline_cg_address([0x82, 0xa0]).unwrap(), expected);
}

#[test]
fn maps_every_external_character_candidate_with_only_bit_seven_different() {
    let candidates = (0x9f..=0xfc)
        .map(|trail| [0xeb, trail])
        .chain((0x40..=0x7e).map(|trail| [0xec, trail]))
        .chain((0x80..=0x9e).map(|trail| [0xec, trail]));
    let mut candidate_count = 0;

    for shift_jis in candidates {
        let addresses = candidate_addresses(shift_jis).unwrap();
        assert!(addresses.same_a1_and_a3_low_7_bits);
        assert!(addresses.a3_bit_7_differs);
        candidate_count += 1;
    }

    assert_eq!(candidate_count, EXTERNAL_CHARACTER_CANDIDATE_CAPACITY);
}

#[test]
fn rejects_invalid_shift_jis_pairs() {
    assert!(drbios_cg_address([0x80, 0x40]).is_err());
    assert!(inline_cg_address([0xeb, 0x7f]).is_err());
}
