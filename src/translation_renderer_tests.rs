use super::{
    INLINE_RENDERERS, assemble_drbios_translation_service, assemble_inline_glyph_loop_hijack,
    assemble_inline_translation_glyph_hook, direct_memory, imm8, imm16, reg8, reg16,
};
use crate::josa::KoreanParticle;
use crate::translation_font::build_translation_font_plan;
use std::collections::BTreeSet;
use v30::{Instruction, OperandSize, Register8, Register16, SegmentRegister, decode_bytes};

#[test]
fn inline_glyph_loop_hijack_is_a_length_preserving_near_jump() {
    for profile in INLINE_RENDERERS {
        let hijack =
            assemble_inline_glyph_loop_hijack(profile.glyph_loop_runtime_offset, 0x6000).unwrap();
        assert_eq!(hijack.len(), 5);
        assert_eq!(hijack[0], 0xe9);
        assert_eq!(&hijack[3..], &[0x90, 0x90]);
    }
}

#[test]
fn inline_translation_hook_accepts_the_banked_rows_produced_by_the_game_converter() {
    let hook = assemble_inline_translation_glyph_hook(0x6000, INLINE_RENDERERS[0]).unwrap();
    let mut instructions = Vec::new();
    let mut remaining = hook.as_slice();
    while !remaining.is_empty() {
        let decoded = decode_bytes(remaining).unwrap();
        instructions.push(decoded.instruction);
        remaining = &remaining[decoded.byte_len..];
    }

    assert!(instructions.contains(&Instruction::Cmp {
        a: reg8(Register8::AH),
        b: imm8(0xf5),
    }));
    assert!(instructions.contains(&Instruction::Cmp {
        a: reg8(Register8::AH),
        b: imm8(0xfe),
    }));
    assert!(instructions.contains(&Instruction::And {
        dest: reg8(Register8::AH),
        src: imm8(0x7f),
    }));
    assert!(
        !instructions.contains(&Instruction::Xchg {
            a: reg8(Register8::AH),
            b: reg8(Register8::AL),
        }),
        "the resident sheet already stores each glyph row left byte first"
    );
}

#[test]
#[ignore = "requires the Galmuri14, NeoDunggeunmo and BMJUA font files under assets/fonts (or MADOU456_ASSET_DIR/fonts)"]
fn resident_glyph_service_selects_subject_particle_from_remembered_batchim() {
    let demand = ['가', '각', KoreanParticle::Subject.marker()]
        .into_iter()
        .collect::<BTreeSet<_>>();
    let font = build_translation_font_plan(&demand, &BTreeSet::new()).unwrap();
    let selector = font.particle_selector();
    let state = 0x3000;
    let service =
        assemble_drbios_translation_service(0x2000, state, 0x4000, 0x5000, selector).unwrap();
    let mut instructions = Vec::new();
    let mut remaining = service.as_slice();
    while !remaining.is_empty() {
        let decoded = decode_bytes(remaining).unwrap();
        instructions.push(decoded.instruction);
        remaining = &remaining[decoded.byte_len..];
    }

    assert!(instructions.contains(&Instruction::Cmp {
        a: reg16(Register16::BX),
        b: imm16(selector.marker_slots()[KoreanParticle::Subject.index()]),
    }));
    assert!(instructions.contains(&Instruction::Mov {
        dest: direct_memory(Some(SegmentRegister::CS), state, OperandSize::Byte),
        src: imm8(1),
    }));
    assert!(instructions.contains(&Instruction::Mov {
        dest: direct_memory(Some(SegmentRegister::CS), state, OperandSize::Byte),
        src: imm8(0),
    }));
    for form_slot in selector.forms(KoreanParticle::Subject) {
        assert!(instructions.contains(&Instruction::Mov {
            dest: reg16(Register16::BX),
            src: imm16(form_slot),
        }));
    }
}
