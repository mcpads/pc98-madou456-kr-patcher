use std::collections::BTreeSet;

use v30::{Instruction, Operand, Register16, decode_bytes};

use super::{
    PRIMARY_MESSAGE_BUFFER_BYTES, SECONDARY_MESSAGE_BUFFER_BYTES,
    mado456_message_buffer_expected_writes, validate_message_buffer_layout,
};
use crate::localization::message_analysis::RebuiltMessageFile;

fn rebuilt_message(name: &str, decoded_size: usize) -> RebuiltMessageFile {
    RebuiltMessageFile {
        name: name.to_owned(),
        decoded: vec![0; decoded_size],
        packed: Vec::new(),
        changed_entry_count: 0,
        source_packed_reproduced: false,
        used_external_characters: BTreeSet::new(),
    }
}

#[test]
fn message_buffers_accept_each_decoded_file_up_to_its_runtime_capacity() {
    let files = [
        rebuilt_message("MSG.DAT", PRIMARY_MESSAGE_BUFFER_BYTES),
        rebuilt_message("MSGEV.DAT", SECONDARY_MESSAGE_BUFFER_BYTES),
        rebuilt_message("MSG01.DAT", 1),
    ];

    validate_message_buffer_layout(&files).unwrap();
}

#[test]
fn message_buffers_reject_primary_and_secondary_overflow() {
    let primary_overflow = [
        rebuilt_message("MSG.DAT", PRIMARY_MESSAGE_BUFFER_BYTES + 1),
        rebuilt_message("MSGEV.DAT", 1),
    ];
    assert!(
        validate_message_buffer_layout(&primary_overflow)
            .unwrap_err()
            .to_string()
            .contains("primary message buffer")
    );

    let secondary_overflow = [
        rebuilt_message("MSG.DAT", 1),
        rebuilt_message("MSGEV.DAT", SECONDARY_MESSAGE_BUFFER_BYTES + 1),
    ];
    assert!(
        validate_message_buffer_layout(&secondary_overflow)
            .unwrap_err()
            .to_string()
            .contains("secondary message buffer")
    );
}

#[test]
fn executable_buffer_writes_preserve_instruction_boundaries_and_operands() {
    let writes = mado456_message_buffer_expected_writes();
    let decoded = writes[..4]
        .iter()
        .map(|write| {
            let decoded = decode_bytes(&write.replacement).unwrap();
            assert_eq!(decoded.byte_len, write.replacement.len());
            decoded.instruction
        })
        .collect::<Vec<_>>();

    assert_eq!(
        decoded[0],
        Instruction::Cmp {
            a: Operand::Reg16(Register16::BX),
            b: Operand::Imm16(0x5500),
        }
    );
    assert_eq!(
        decoded[1],
        Instruction::Mov {
            dest: Operand::Reg16(Register16::BX),
            src: Operand::Imm16(0x5500),
        }
    );
    assert_eq!(
        decoded[2],
        Instruction::Add {
            dest: Operand::Reg16(Register16::AX),
            src: Operand::Imm16(0x04c0),
        }
    );
    assert_eq!(
        decoded[3],
        Instruction::Add {
            dest: Operand::Reg16(Register16::DX),
            src: Operand::Imm16(0x01b0),
        }
    );
    assert_eq!(writes[4].replacement, 0x1b00_u16.to_le_bytes());
}
