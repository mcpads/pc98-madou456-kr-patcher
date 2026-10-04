use anyhow::{Context, Result, ensure};
use serde::Serialize;
use sha2::{Digest, Sha256};
use v30::{
    Assembler, CodeLocation, Condition, EffectiveAddress, EffectiveAddressBase,
    EffectiveAddressDisplacement, Instruction, JmpTarget, Operand, OperandSize, Register8,
    Register16, SegmentRegister, ShiftCount,
};

use crate::expected_write::{FixedRangeExpectedWrite, apply_fixed_range_expected_writes};
use crate::josa::KoreanParticle;
use crate::message_buffers::mado456_message_buffer_expected_writes;
use crate::translation_font::{
    TRANSLATION_GLYPH_CAPACITY, TranslationFontPlan, TranslationParticleSelector,
};

const COM_LOAD_ORIGIN: usize = 0x100;
const DRBIOS_SHA256: &str = "6762cd03975e26c07f98b78632adf8418d3648a518a0fbfc4815e96ae5b0d5f8";
const MADO456_SHA256: &str = "df31cddd940f33169a5c812a68000258ec0350cdcecbe4fdb036a5dc30597966";
const OPENING_SHA256: &str = "c9dc5839685f9a4d505659b656594bee8948d852e5d00ed657a6a71037d10db2";
const ENDING_SHA256: &str = "a95d2a854eb813adea064233a7215fdbae2e7461b732957bef9e1b7464315460";

const DRBIOS_DISPATCH_LIMIT_FILE_OFFSET: usize = 0x002d;
const DRBIOS_SERVICE_34_POINTER_FILE_OFFSET: usize = 0x00a1;
const DRBIOS_RESIDENT_ADDEND_FILE_OFFSET: usize = 0x245b;
const DRBIOS_RESIDENT_BASE_PARAGRAPHS: usize = 0x24a;
const DRBIOS_SAVED_STACK_SEGMENT_RUNTIME_OFFSET: u16 = 0x1f67;
const DRBIOS_SAVED_STACK_POINTER_RUNTIME_OFFSET: u16 = 0x1f65;
const DRBIOS_ORIGINAL_GLYPH_RUNTIME_OFFSET: u16 = 0x078c;
const DRBIOS_ORIGINAL_GLYPH_RETURN_RUNTIME_OFFSET: u16 = 0x0776;
const TRANSLATION_GLYPH_SERVICE: u8 = 0x34;

const MADO456_GLYPH_SERVICE_FILE_OFFSET: usize = 0x279b;
const INLINE_TRANSLATION_BANKED_FIRST_ROW: u8 = 0xf5;
const INLINE_TRANSLATION_BANKED_LAST_ROW: u8 = 0xfe;
const INLINE_TRANSLATION_ROW_MASK: u8 = 0x7f;

#[derive(Clone, Copy)]
struct InlineRendererProfile {
    name: &'static str,
    sha256: &'static str,
    glyph_loop_file_offset: usize,
    glyph_loop_runtime_offset: u16,
    original_glyph_runtime_offset: u16,
    glyph_loop_end_runtime_offset: u16,
    draw_glyph_runtime_offset: u16,
    glyph_row_expander_runtime_offset: Option<u16>,
}

const INLINE_RENDERERS: [InlineRendererProfile; 3] = [
    InlineRendererProfile {
        name: "OPENING.COM",
        sha256: OPENING_SHA256,
        glyph_loop_file_offset: 0x2605,
        glyph_loop_runtime_offset: 0x2705,
        original_glyph_runtime_offset: 0x270a,
        glyph_loop_end_runtime_offset: 0x2741,
        draw_glyph_runtime_offset: 0x275c,
        glyph_row_expander_runtime_offset: None,
    },
    InlineRendererProfile {
        name: "ENDING.COM",
        sha256: ENDING_SHA256,
        glyph_loop_file_offset: 0x3ad6,
        glyph_loop_runtime_offset: 0x3bd6,
        original_glyph_runtime_offset: 0x3bdb,
        glyph_loop_end_runtime_offset: 0x3c12,
        draw_glyph_runtime_offset: 0x3c2d,
        glyph_row_expander_runtime_offset: None,
    },
    InlineRendererProfile {
        name: "ENDING.COM",
        sha256: ENDING_SHA256,
        glyph_loop_file_offset: 0x307f,
        glyph_loop_runtime_offset: 0x317f,
        original_glyph_runtime_offset: 0x3184,
        glyph_loop_end_runtime_offset: 0x31b9,
        draw_glyph_runtime_offset: 0x31d4,
        glyph_row_expander_runtime_offset: Some(0x31fd),
    },
];

#[derive(Debug, Clone, Eq, PartialEq, Serialize)]
pub(super) struct RendererPatchReport {
    pub(super) program: String,
    pub(super) original_sha256: String,
    pub(super) updated_sha256: String,
    pub(super) original_size: usize,
    pub(super) updated_size: usize,
    pub(super) fixed_expected_write_count: usize,
    pub(super) appended_offset: usize,
    pub(super) appended_size: usize,
}

pub(super) fn install_drbios_translation_font(
    source: &[u8],
    font: &TranslationFontPlan,
) -> Result<(Vec<u8>, RendererPatchReport)> {
    require_program("DRBIOS.COM", source, DRBIOS_SHA256)?;
    ensure!(
        font.occupancy().len() == TRANSLATION_GLYPH_CAPACITY,
        "translation font occupancy has the wrong size"
    );

    let service_runtime_offset = runtime_offset(source.len())?;
    let provisional = assemble_drbios_translation_service(
        service_runtime_offset,
        0x4000,
        0x5000,
        0x6000,
        font.particle_selector(),
    )?;
    let particle_state_runtime_offset = service_runtime_offset
        .checked_add(u16::try_from(provisional.len())?)
        .context("DRBIOS particle state address overflow")?;
    let occupancy_runtime_offset = particle_state_runtime_offset
        .checked_add(1)
        .context("DRBIOS translation occupancy address overflow")?;
    let font_runtime_offset = occupancy_runtime_offset
        .checked_add(u16::try_from(font.occupancy().len())?)
        .context("DRBIOS translation font address overflow")?;
    let service = assemble_drbios_translation_service(
        service_runtime_offset,
        particle_state_runtime_offset,
        occupancy_runtime_offset,
        font_runtime_offset,
        font.particle_selector(),
    )?;
    ensure!(
        service.len() == provisional.len(),
        "DRBIOS translation service changed size after placement"
    );

    let appended_size = service
        .len()
        .checked_add(1)
        .and_then(|size| size.checked_add(font.occupancy().len()))
        .and_then(|size| size.checked_add(font.sheet().len()))
        .context("DRBIOS translation payload size overflow")?;
    let final_size = source
        .len()
        .checked_add(appended_size)
        .context("DRBIOS translated size overflow")?;
    let final_runtime_end = final_size
        .checked_add(COM_LOAD_ORIGIN)
        .context("DRBIOS translated runtime end overflow")?;
    ensure!(
        final_runtime_end <= 0x1_0000,
        "DRBIOS translation font exceeds the COM segment"
    );
    let resident_paragraphs = 0x10usize
        .checked_add(final_runtime_end.div_ceil(16))
        .context("DRBIOS resident paragraph count overflow")?;
    let resident_addend = resident_paragraphs
        .checked_sub(DRBIOS_RESIDENT_BASE_PARAGRAPHS)
        .context("DRBIOS resident paragraph plan is below its fixed base")?;
    let resident_addend = u16::try_from(resident_addend)
        .context("DRBIOS resident paragraph addend exceeds 16 bits")?;

    let writes = [
        FixedRangeExpectedWrite {
            owner: "drbios-translation-service",
            purpose: "admit the appended AH=34h glyph service",
            offset: DRBIOS_DISPATCH_LIMIT_FILE_OFFSET,
            expected_source: vec![0x34],
            replacement: vec![0x35],
        },
        FixedRangeExpectedWrite {
            owner: "drbios-translation-service",
            purpose: "bind AH=34h to the appended translation glyph reader",
            offset: DRBIOS_SERVICE_34_POINTER_FILE_OFFSET,
            expected_source: vec![0, 0],
            replacement: service_runtime_offset.to_le_bytes().to_vec(),
        },
        FixedRangeExpectedWrite {
            owner: "drbios-translation-font-memory",
            purpose: "keep the appended occupancy map and glyph sheet resident",
            offset: DRBIOS_RESIDENT_ADDEND_FILE_OFFSET,
            expected_source: 0x0180_u16.to_le_bytes().to_vec(),
            replacement: resident_addend.to_le_bytes().to_vec(),
        },
    ];
    let mut updated = apply_fixed_range_expected_writes(source, &writes)?;
    updated.extend_from_slice(&service);
    updated.push(0);
    updated.extend_from_slice(font.occupancy());
    updated.extend_from_slice(font.sheet());
    ensure!(
        updated.len() == final_size,
        "DRBIOS translation payload changed size after writing"
    );
    ensure!(
        updated.get(source.len()..source.len() + service.len()) == Some(service.as_slice()),
        "DRBIOS appended translation service readback differs"
    );

    Ok((
        updated.clone(),
        RendererPatchReport {
            program: "DRBIOS.COM".to_owned(),
            original_sha256: sha256_hex(source),
            updated_sha256: sha256_hex(&updated),
            original_size: source.len(),
            updated_size: updated.len(),
            fixed_expected_write_count: writes.len(),
            appended_offset: source.len(),
            appended_size,
        },
    ))
}

pub(super) fn install_mado456_localization_runtime_support(
    source: &[u8],
) -> Result<(Vec<u8>, RendererPatchReport)> {
    require_program("MADO456.COM", source, MADO456_SHA256)?;
    let mut writes = vec![FixedRangeExpectedWrite {
        owner: "mado456-translation-glyph-route",
        purpose: "route every message glyph through the fallback-preserving translation service",
        offset: MADO456_GLYPH_SERVICE_FILE_OFFSET,
        expected_source: vec![0x16],
        replacement: vec![TRANSLATION_GLYPH_SERVICE],
    }];
    writes.extend(mado456_message_buffer_expected_writes());
    let updated = apply_fixed_range_expected_writes(source, &writes)?;
    Ok((
        updated.clone(),
        RendererPatchReport {
            program: "MADO456.COM".to_owned(),
            original_sha256: sha256_hex(source),
            updated_sha256: sha256_hex(&updated),
            original_size: source.len(),
            updated_size: updated.len(),
            fixed_expected_write_count: writes.len(),
            appended_offset: source.len(),
            appended_size: 0,
        },
    ))
}

pub(super) fn route_inline_renderer_through_translation_glyph_service(
    name: &str,
    source: &[u8],
) -> Result<(Vec<u8>, RendererPatchReport)> {
    let profiles: Vec<_> = INLINE_RENDERERS
        .iter()
        .filter(|profile| profile.name == name)
        .copied()
        .collect();
    let first = profiles
        .first()
        .with_context(|| format!("unsupported inline renderer {name}"))?;
    require_program(name, source, first.sha256)?;
    let mut hooks = Vec::new();
    let mut writes = Vec::new();
    for profile in profiles {
        let hook_runtime_offset = runtime_offset(source.len() + hooks.len())?;
        let hook = assemble_inline_translation_glyph_hook(hook_runtime_offset, profile)?;
        let hijack = assemble_inline_glyph_loop_hijack(
            profile.glyph_loop_runtime_offset,
            hook_runtime_offset,
        )?;
        ensure!(
            hijack.len() == 5,
            "inline glyph-loop hijack is not length preserving"
        );
        let end_displacement = u8::try_from(
            profile.glyph_loop_end_runtime_offset - profile.glyph_loop_runtime_offset - 5,
        )?;
        writes.push(FixedRangeExpectedWrite {
            owner: "inline-translation-glyph-route",
            purpose: "redirect each inline CG reader through the resident translation glyph service",
            offset: profile.glyph_loop_file_offset,
            expected_source: vec![0xad, 0x85, 0xc0, 0x74, end_displacement],
            replacement: hijack,
        });
        hooks.extend_from_slice(&hook);
    }
    let mut updated = apply_fixed_range_expected_writes(source, &writes)?;
    updated.extend_from_slice(&hooks);
    ensure!(
        updated.len() + COM_LOAD_ORIGIN <= 0x1_0000,
        "{} translation hook exceeds the COM segment",
        name
    );
    ensure!(
        updated.get(source.len()..) == Some(hooks.as_slice()),
        "{} appended translation hook readback differs",
        name
    );
    Ok((
        updated.clone(),
        RendererPatchReport {
            program: name.to_owned(),
            original_sha256: sha256_hex(source),
            updated_sha256: sha256_hex(&updated),
            original_size: source.len(),
            updated_size: updated.len(),
            fixed_expected_write_count: writes.len(),
            appended_offset: source.len(),
            appended_size: hooks.len(),
        },
    ))
}

fn assemble_drbios_translation_service(
    origin: u16,
    particle_state: u16,
    occupancy: u16,
    font: u16,
    particle_selector: &TranslationParticleSelector,
) -> Result<Vec<u8>> {
    let mut assembler = Assembler::new();
    assembler
        .emit(Instruction::Cmp {
            a: reg8(Register8::DH),
            b: imm8(0xeb),
        })
        .emit_branch(Condition::B, "fallback_trampoline")
        .emit(Instruction::Cmp {
            a: reg8(Register8::DH),
            b: imm8(0xef),
        })
        .emit_branch(Condition::A, "fallback_trampoline")
        .emit(Instruction::Cmp {
            a: reg8(Register8::DL),
            b: imm8(0x40),
        })
        .emit_branch(Condition::B, "fallback_trampoline")
        .emit(Instruction::Cmp {
            a: reg8(Register8::DL),
            b: imm8(0xfc),
        })
        .emit_branch(Condition::A, "fallback_trampoline")
        .emit(Instruction::Cmp {
            a: reg8(Register8::DL),
            b: imm8(0x7f),
        })
        .emit_branch(Condition::E, "fallback_trampoline")
        .emit(Instruction::Mov {
            dest: reg8(Register8::AL),
            src: reg8(Register8::DH),
        })
        .emit(Instruction::Sub {
            dest: reg8(Register8::AL),
            src: imm8(0xeb),
        })
        .emit(Instruction::Mov {
            dest: reg8(Register8::AH),
            src: imm8(188),
        })
        .emit(Instruction::Mul {
            src: reg8(Register8::AH),
        })
        .emit(Instruction::Mov {
            dest: reg16(Register16::BX),
            src: reg16(Register16::AX),
        })
        .emit(Instruction::Cmp {
            a: reg8(Register8::DL),
            b: imm8(0x9f),
        })
        .emit_branch(Condition::B, "odd_row")
        .emit(Instruction::Mov {
            dest: reg8(Register8::AL),
            src: reg8(Register8::DL),
        })
        .emit(Instruction::Sub {
            dest: reg8(Register8::AL),
            src: imm8(0x9f),
        })
        .emit(Instruction::Xor {
            dest: reg8(Register8::AH),
            src: reg8(Register8::AH),
        })
        .emit(Instruction::Add {
            dest: reg16(Register16::AX),
            src: imm16(94),
        })
        .emit_jump_near("add_cell")
        .label("odd_row")
        .emit(Instruction::Mov {
            dest: reg8(Register8::AL),
            src: reg8(Register8::DL),
        })
        .emit(Instruction::Sub {
            dest: reg8(Register8::AL),
            src: imm8(0x40),
        })
        .emit(Instruction::Cmp {
            a: reg8(Register8::DL),
            b: imm8(0x80),
        })
        .emit_branch(Condition::B, "odd_cell_ready")
        .emit(Instruction::Dec {
            dest: reg8(Register8::AL),
        })
        .label("odd_cell_ready")
        .emit(Instruction::Xor {
            dest: reg8(Register8::AH),
            src: reg8(Register8::AH),
        })
        .label("add_cell")
        .emit(Instruction::Add {
            dest: reg16(Register16::BX),
            src: reg16(Register16::AX),
        })
        .emit(Instruction::Cmp {
            a: based_memory(
                Some(SegmentRegister::CS),
                EffectiveAddressBase::Bx,
                i16::try_from(occupancy).context("DRBIOS occupancy displacement exceeds i16")?,
                OperandSize::Byte,
            ),
            b: imm8(0),
        })
        .emit_branch(Condition::E, "fallback_trampoline")
        .emit_jump_near("translation_glyph")
        .label("fallback_trampoline")
        .emit_jump_near("fallback")
        .label("translation_glyph")
        .emit(Instruction::Cmp {
            a: reg16(Register16::BX),
            b: imm16(particle_selector.marker_slots()[KoreanParticle::Object.index()]),
        })
        .emit_branch(Condition::E, "select_object_particle")
        .emit(Instruction::Cmp {
            a: reg16(Register16::BX),
            b: imm16(particle_selector.marker_slots()[KoreanParticle::Subject.index()]),
        })
        .emit_branch(Condition::E, "select_subject_particle")
        .emit(Instruction::Cmp {
            a: reg16(Register16::BX),
            b: imm16(particle_selector.marker_slots()[KoreanParticle::Topic.index()]),
        })
        .emit_branch(Condition::E, "select_topic_particle")
        .emit(Instruction::Cmp {
            a: reg16(Register16::BX),
            b: imm16(particle_selector.marker_slots()[KoreanParticle::With.index()]),
        })
        .emit_branch(Condition::E, "select_with_particle")
        .emit(Instruction::Cmp {
            a: reg16(Register16::BX),
            b: imm16(particle_selector.first_hangul_slot()),
        })
        .emit_branch(Condition::B, "symbol_glyph")
        .emit_jump_near("classify_hangul")
        .label("symbol_glyph")
        .emit_jump_near("glyph_ready")
        .label("classify_hangul")
        .emit(Instruction::Cmp {
            a: reg16(Register16::BX),
            b: imm16(particle_selector.first_with_batchim_slot()),
        })
        .emit_branch(Condition::B, "remember_without_batchim")
        .emit(Instruction::Mov {
            dest: direct_memory(Some(SegmentRegister::CS), particle_state, OperandSize::Byte),
            src: imm8(1),
        })
        .emit_jump_near("glyph_ready")
        .label("remember_without_batchim")
        .emit(Instruction::Mov {
            dest: direct_memory(Some(SegmentRegister::CS), particle_state, OperandSize::Byte),
            src: imm8(0),
        })
        .emit_jump_near("glyph_ready")
        .label("select_object_particle");
    emit_particle_selection(
        &mut assembler,
        "object_with_batchim",
        particle_state,
        particle_selector.forms(KoreanParticle::Object),
    );
    assembler.label("select_subject_particle");
    emit_particle_selection(
        &mut assembler,
        "subject_with_batchim",
        particle_state,
        particle_selector.forms(KoreanParticle::Subject),
    );
    assembler.label("select_topic_particle");
    emit_particle_selection(
        &mut assembler,
        "topic_with_batchim",
        particle_state,
        particle_selector.forms(KoreanParticle::Topic),
    );
    assembler.label("select_with_particle");
    emit_particle_selection(
        &mut assembler,
        "with_with_batchim",
        particle_state,
        particle_selector.forms(KoreanParticle::With),
    );
    assembler.label("glyph_ready").emit(Instruction::Mov {
        dest: reg16(Register16::AX),
        src: reg16(Register16::BX),
    });
    for _ in 0..5 {
        assembler.emit(Instruction::Shl {
            dest: reg16(Register16::AX),
            count: ShiftCount::One,
        });
    }
    assembler
        .emit(Instruction::Add {
            dest: reg16(Register16::AX),
            src: imm16(font),
        })
        .emit(Instruction::Mov {
            dest: segment(SegmentRegister::ES),
            src: direct_memory(
                Some(SegmentRegister::CS),
                DRBIOS_SAVED_STACK_SEGMENT_RUNTIME_OFFSET,
                OperandSize::Word,
            ),
        })
        .emit(Instruction::Mov {
            dest: reg16(Register16::DI),
            src: direct_memory(
                Some(SegmentRegister::CS),
                DRBIOS_SAVED_STACK_POINTER_RUNTIME_OFFSET,
                OperandSize::Word,
            ),
        })
        .emit(Instruction::Mov {
            dest: based_memory(
                Some(SegmentRegister::ES),
                EffectiveAddressBase::Di,
                2,
                OperandSize::Word,
            ),
            src: segment(SegmentRegister::CS),
        })
        .emit(Instruction::Mov {
            dest: based_memory(
                Some(SegmentRegister::ES),
                EffectiveAddressBase::Di,
                0x0e,
                OperandSize::Word,
            ),
            src: reg16(Register16::AX),
        })
        .emit(Instruction::Ret { pop: 0 })
        .label("fallback")
        .emit_call_near("original_glyph")
        .emit_jump_near("original_return")
        .label("original_glyph")
        .emit(Instruction::Push {
            src: imm16(DRBIOS_ORIGINAL_GLYPH_RUNTIME_OFFSET),
        })
        .emit(Instruction::Ret { pop: 0 })
        .label("original_return")
        .emit(Instruction::Push {
            src: imm16(DRBIOS_ORIGINAL_GLYPH_RETURN_RUNTIME_OFFSET),
        })
        .emit(Instruction::Ret { pop: 0 });
    assemble_at(&assembler, origin, "DRBIOS translation glyph service")
}

fn emit_particle_selection(
    assembler: &mut Assembler,
    with_batchim_label: &'static str,
    particle_state: u16,
    forms: [u16; 2],
) {
    assembler
        .emit(Instruction::Cmp {
            a: direct_memory(Some(SegmentRegister::CS), particle_state, OperandSize::Byte),
            b: imm8(0),
        })
        .emit_branch(Condition::Ne, with_batchim_label)
        .emit(Instruction::Mov {
            dest: reg16(Register16::BX),
            src: imm16(forms[0]),
        })
        .emit_jump_near("glyph_ready")
        .label(with_batchim_label)
        .emit(Instruction::Mov {
            dest: reg16(Register16::BX),
            src: imm16(forms[1]),
        })
        .emit_jump_near("glyph_ready");
}

fn assemble_inline_translation_glyph_hook(
    origin: u16,
    profile: InlineRendererProfile,
) -> Result<Vec<u8>> {
    let mut assembler = Assembler::new();
    assembler
        .label("glyph_loop")
        .emit(Instruction::Lodsw)
        .emit(Instruction::Test {
            a: reg16(Register16::AX),
            b: reg16(Register16::AX),
        })
        .emit_branch(Condition::E, "glyph_loop_end")
        .emit(Instruction::Cmp {
            a: reg8(Register8::AH),
            b: imm8(INLINE_TRANSLATION_BANKED_FIRST_ROW),
        })
        .emit_branch(Condition::B, "original_glyph")
        .emit(Instruction::Cmp {
            a: reg8(Register8::AH),
            b: imm8(INLINE_TRANSLATION_BANKED_LAST_ROW),
        })
        .emit_branch(Condition::A, "original_glyph")
        .emit(Instruction::Push {
            src: reg16(Register16::DI),
        })
        .emit(Instruction::Push {
            src: reg16(Register16::SI),
        })
        .emit(Instruction::Push {
            src: reg16(Register16::DX),
        })
        .emit(Instruction::And {
            dest: reg8(Register8::AH),
            src: imm8(INLINE_TRANSLATION_ROW_MASK),
        })
        .emit(Instruction::Mov {
            dest: reg8(Register8::BL),
            src: reg8(Register8::AH),
        })
        .emit(Instruction::Mov {
            dest: reg8(Register8::DL),
            src: reg8(Register8::AL),
        })
        .emit(Instruction::Sub {
            dest: reg8(Register8::BL),
            src: imm8(0x75),
        })
        .emit(Instruction::Shr {
            dest: reg8(Register8::BL),
            count: ShiftCount::One,
        })
        .emit(Instruction::Add {
            dest: reg8(Register8::BL),
            src: imm8(0xeb),
        })
        .emit(Instruction::Test {
            a: reg8(Register8::AH),
            b: imm8(1),
        })
        .emit_branch(Condition::E, "even_row")
        .emit(Instruction::Add {
            dest: reg8(Register8::DL),
            src: imm8(0x1f),
        })
        .emit(Instruction::Cmp {
            a: reg8(Register8::AL),
            b: imm8(0x60),
        })
        .emit_branch(Condition::B, "pair_ready")
        .emit(Instruction::Inc {
            dest: reg8(Register8::DL),
        })
        .emit_jump_near("pair_ready")
        .label("even_row")
        .emit(Instruction::Add {
            dest: reg8(Register8::DL),
            src: imm8(0x7e),
        })
        .label("pair_ready")
        .emit(Instruction::Mov {
            dest: reg8(Register8::DH),
            src: reg8(Register8::BL),
        })
        .emit(Instruction::Mov {
            dest: reg8(Register8::AH),
            src: imm8(TRANSLATION_GLYPH_SERVICE),
        })
        .emit(Instruction::Int { vector: 0x64 })
        .emit(Instruction::Mov {
            dest: reg16(Register16::SI),
            src: reg16(Register16::DX),
        })
        .emit(Instruction::Mov {
            dest: reg8(Register8::CH),
            src: imm8(16),
        })
        .label("glyph_row")
        .emit(Instruction::Lodsw);
    if profile.glyph_row_expander_runtime_offset.is_some() {
        // The credits' byte-doubling table belongs to CS, while glyph rows
        // are read through the resident service's returned DS:SI.
        assembler
            .emit(Instruction::Push {
                src: segment(SegmentRegister::DS),
            })
            .emit(Instruction::Push {
                src: segment(SegmentRegister::CS),
            })
            .emit(Instruction::Pop {
                dest: segment(SegmentRegister::DS),
            })
            .emit_call_near("expand_glyph_row")
            .emit(Instruction::Pop {
                dest: segment(SegmentRegister::DS),
            });
    } else {
        assembler
            .emit(Instruction::Mov {
                dest: reg16(Register16::BX),
                src: reg16(Register16::AX),
            })
            .emit(Instruction::Rol {
                dest: reg16(Register16::AX),
                count: ShiftCount::One,
            })
            .emit(Instruction::Or {
                dest: reg16(Register16::AX),
                src: reg16(Register16::BX),
            })
            .emit(Instruction::Stosw);
    }
    assembler
        .emit(Instruction::Dec {
            dest: reg8(Register8::CH),
        })
        .emit_branch(Condition::Ne, "glyph_row")
        .emit(Instruction::Pop {
            dest: reg16(Register16::DX),
        })
        .emit(Instruction::Push {
            src: segment(SegmentRegister::CS),
        })
        .emit(Instruction::Pop {
            dest: segment(SegmentRegister::DS),
        })
        .emit_call_near("draw_glyph")
        .emit(Instruction::Pop {
            dest: reg16(Register16::SI),
        })
        .emit(Instruction::Pop {
            dest: reg16(Register16::DI),
        })
        .emit(Instruction::Dec {
            dest: reg8(Register8::CL),
        })
        .emit_branch(Condition::Ne, "glyph_loop")
        .emit_jump_near("glyph_loop_end")
        .label("original_glyph")
        .emit(Instruction::Push {
            src: imm16(profile.original_glyph_runtime_offset),
        })
        .emit(Instruction::Ret { pop: 0 })
        .label("glyph_loop_end")
        .emit(Instruction::Push {
            src: imm16(profile.glyph_loop_end_runtime_offset),
        })
        .emit(Instruction::Ret { pop: 0 })
        .label("draw_glyph")
        .emit(Instruction::Push {
            src: imm16(profile.draw_glyph_runtime_offset),
        })
        .emit(Instruction::Ret { pop: 0 });
    if let Some(expander) = profile.glyph_row_expander_runtime_offset {
        assembler
            .label("expand_glyph_row")
            .emit(Instruction::Push {
                src: imm16(expander),
            })
            .emit(Instruction::Ret { pop: 0 });
    }
    assemble_at(&assembler, origin, "inline translation glyph hook")
}

fn assemble_inline_glyph_loop_hijack(origin: u16, hook: u16) -> Result<Vec<u8>> {
    let mut assembler = Assembler::new();
    assembler.emit(Instruction::Jmp {
        target: JmpTarget::Rel16(near_displacement(origin, hook)),
    });
    assembler.emit(Instruction::Nop).emit(Instruction::Nop);
    assemble_at(&assembler, origin, "inline translation glyph-loop hijack")
}

fn assemble_at(assembler: &Assembler, origin: u16, purpose: &str) -> Result<Vec<u8>> {
    assembler
        .assemble(CodeLocation {
            seg: 0,
            off: origin,
        })
        .with_context(|| format!("assemble typed V30 {purpose} at 0x{origin:04X}"))
        .map(|program| program.bytes().to_vec())
}

fn based_memory(
    segment: Option<SegmentRegister>,
    base: EffectiveAddressBase,
    displacement: i16,
    size: OperandSize,
) -> Operand {
    Operand::Mem(
        EffectiveAddress::new(
            segment,
            base,
            EffectiveAddressDisplacement::Signed(displacement),
            size,
        )
        .expect("translation renderer address is representable"),
    )
}

fn direct_memory(segment: Option<SegmentRegister>, address: u16, size: OperandSize) -> Operand {
    Operand::Mem(
        EffectiveAddress::new(
            segment,
            EffectiveAddressBase::Direct,
            EffectiveAddressDisplacement::Absolute(address),
            size,
        )
        .expect("translation renderer direct address is representable"),
    )
}

const fn reg8(register: Register8) -> Operand {
    Operand::Reg8(register)
}

const fn reg16(register: Register16) -> Operand {
    Operand::Reg16(register)
}

const fn segment(register: SegmentRegister) -> Operand {
    Operand::Sreg(register)
}

const fn imm8(value: u8) -> Operand {
    Operand::Imm8(value)
}

const fn imm16(value: u16) -> Operand {
    Operand::Imm16(value)
}

const fn near_displacement(origin: u16, target: u16) -> i16 {
    target.wrapping_sub(origin.wrapping_add(3)) as i16
}

fn runtime_offset(file_offset: usize) -> Result<u16> {
    u16::try_from(
        file_offset
            .checked_add(COM_LOAD_ORIGIN)
            .context("COM runtime offset overflow")?,
    )
    .context("COM runtime offset exceeds 16 bits")
}

fn require_program(name: &str, source: &[u8], expected_sha256: &str) -> Result<()> {
    ensure!(
        sha256_hex(source) == expected_sha256,
        "{name} differs from the supported translation renderer preimage"
    );
    Ok(())
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut digest = Sha256::new();
    digest.update(bytes);
    digest
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[cfg(test)]
#[path = "translation_renderer_tests.rs"]
mod tests;
