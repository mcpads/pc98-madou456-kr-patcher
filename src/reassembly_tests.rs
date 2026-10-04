use std::collections::BTreeMap;

use super::{AUTOEXEC_BYTES, RETAINED_ROOT_NAMES, apply_game_file_replacements, sha256_hex};
use crate::source_cd::{GameFile, encode_short_name};

#[test]
fn standalone_launch_script_uses_only_retained_paths_and_cd_game_launcher() {
    let text = String::from_utf8_lossy(AUTOEXEC_BYTES);
    assert!(text.contains("PATH A:\\DOS;A:\\MADOU456;A:\\"));
    assert!(text.contains("CD \\MADOU456\r\nCALL 456.BAT"));
    assert!(!text.contains("SMENU"));
}

#[test]
fn retained_root_profile_contains_the_boot_and_dos_dependencies() {
    assert!(RETAINED_ROOT_NAMES.contains(&"IO.SYS"));
    assert!(RETAINED_ROOT_NAMES.contains(&"MSDOS.SYS"));
    assert!(RETAINED_ROOT_NAMES.contains(&"COMMAND.COM"));
    assert!(RETAINED_ROOT_NAMES.contains(&"DOS"));
    assert!(RETAINED_ROOT_NAMES.contains(&"CONFIG.SYS"));
}

#[test]
fn hash_format_is_lowercase_sha256() {
    assert_eq!(
        sha256_hex(b"abc"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
}

#[test]
fn replacements_keep_source_identity_and_replace_only_named_payloads() {
    let source = vec![
        GameFile {
            display_name: "A.DAT".to_owned(),
            short_name: encode_short_name("A.DAT").unwrap(),
            bytes: vec![1],
        },
        GameFile {
            display_name: "B.DAT".to_owned(),
            short_name: encode_short_name("B.DAT").unwrap(),
            bytes: vec![2],
        },
    ];
    let replacements = BTreeMap::from([("B.DAT".to_owned(), vec![9, 8])]);

    let updated = apply_game_file_replacements(&source, &replacements).unwrap();

    assert_eq!(updated[0], source[0]);
    assert_eq!(updated[1].display_name, "B.DAT");
    assert_eq!(updated[1].short_name, source[1].short_name);
    assert_eq!(updated[1].bytes, [9, 8]);
}

#[test]
fn replacement_for_unverified_filename_is_rejected() {
    let source = vec![GameFile {
        display_name: "A.DAT".to_owned(),
        short_name: encode_short_name("A.DAT").unwrap(),
        bytes: vec![1],
    }];
    let replacements = BTreeMap::from([("OTHER.DAT".to_owned(), vec![9])]);

    assert!(apply_game_file_replacements(&source, &replacements).is_err());
}
