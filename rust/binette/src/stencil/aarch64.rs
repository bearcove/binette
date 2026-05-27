use super::*;
use crate::layout::{option_string_layout, string_layout};

pub(super) fn generate_code(
    ops: &[StencilOp],
    failure_count: usize,
) -> Result<ExecutableMemory, StencilError> {
    #[cfg(all(target_arch = "aarch64", target_endian = "little"))]
    {
        let mut code = Vec::with_capacity(ops.len() * 16 + (failure_count + 1) * 8);
        let mut branches = Vec::new();
        for op in ops {
            emit_op(&mut code, op, &mut branches)?;
        }
        push_u32(&mut code, mov_w0_immediate(STENCIL_OK)?);
        push_u32(&mut code, AARCH64_RET);

        let mut failure_offsets = Vec::with_capacity(failure_count);
        for index in 0..failure_count {
            failure_offsets.push(code.len());
            push_u32(&mut code, mov_w0_immediate(status_for_failure(index)?)?);
            push_u32(&mut code, AARCH64_RET);
        }
        for branch in branches {
            let Some(target) = failure_offsets.get(branch.failure_index).copied() else {
                return Err(StencilError::Unsupported {
                    path: "$code".to_owned(),
                    reason: "stencil branch references missing failure stub",
                });
            };
            let word = match branch.kind {
                BranchKind::CondHi => patch_cond_branch_imm19(AARCH64_B_HI, branch.offset, target)?,
                BranchKind::Uncond => patch_uncond_branch_imm26(AARCH64_B, branch.offset, target)?,
            };
            code[branch.offset..branch.offset + 4].copy_from_slice(&word.to_le_bytes());
        }
        ExecutableMemory::new(&code)
    }

    #[cfg(not(all(target_arch = "aarch64", target_endian = "little")))]
    {
        let _ = ops;
        let _ = failure_count;
        Err(StencilError::Unsupported {
            path: "$".to_owned(),
            reason: "stencil backend currently requires little-endian AArch64",
        })
    }
}

pub(super) fn generate_direct_encode_code(
    ops: &[CopyOp],
    output_len: usize,
) -> Result<ExecutableMemory, StencilError> {
    #[cfg(all(target_arch = "aarch64", target_endian = "little"))]
    {
        let mut code = Vec::with_capacity(ops.len() * 8 + 128);
        emit_direct_encode_prologue(&mut code);

        push_u32(&mut code, mov_x_register(0, 20)?);
        let output_len = u64::try_from(output_len).map_err(|_| StencilError::Unsupported {
            path: "$code".to_owned(),
            reason: "direct encode output length exceeds u64",
        })?;
        emit_mov_x_immediate(&mut code, 1, output_len)?;
        emit_mov_x_immediate(
            &mut code,
            16,
            stencil_encode_reserve as *const () as usize as u64,
        )?;
        push_u32(&mut code, AARCH64_BLR_X16);

        let reserve_failed_branch = code.len();
        push_u32(&mut code, 0);

        push_u32(&mut code, mov_x_register(21, 0)?);
        push_u32(&mut code, mov_x_register(0, 19)?);
        push_u32(&mut code, mov_x_register(2, 21)?);
        for op in ops {
            emit_copy_op(&mut code, *op)?;
        }

        push_u32(&mut code, mov_w0_immediate(STENCIL_OK)?);
        let success_branch = code.len();
        push_u32(&mut code, 0);

        let reserve_failed = code.len();
        push_u32(&mut code, mov_w0_immediate(1)?);

        let epilogue = code.len();
        emit_direct_encode_epilogue(&mut code);

        let reserve_failed_word =
            patch_compare_zero_branch_imm19(AARCH64_CBZ_X0, reserve_failed_branch, reserve_failed)?;
        code[reserve_failed_branch..reserve_failed_branch + 4]
            .copy_from_slice(&reserve_failed_word.to_le_bytes());

        let success_word = patch_uncond_branch_imm26(AARCH64_B, success_branch, epilogue)?;
        code[success_branch..success_branch + 4].copy_from_slice(&success_word.to_le_bytes());

        ExecutableMemory::new(&code)
    }

    #[cfg(not(all(target_arch = "aarch64", target_endian = "little")))]
    {
        let _ = ops;
        let _ = output_len;
        Err(StencilError::Unsupported {
            path: "$".to_owned(),
            reason: "stencil backend currently requires little-endian AArch64",
        })
    }
}

pub(super) fn generate_hybrid_code(
    ops: &[HybridStencilOp],
) -> Result<ExecutableMemory, StencilError> {
    #[cfg(all(target_arch = "aarch64", target_endian = "little"))]
    {
        let mut code = Vec::with_capacity(ops.len() * 48 + 128);
        emit_hybrid_prologue(&mut code);

        let mut error_branches = Vec::new();
        for op in ops {
            emit_hybrid_op(&mut code, op, &mut error_branches)?;
        }

        push_u32(&mut code, AARCH64_MOV_X0_X23);
        let epilogue_offset = code.len();
        emit_hybrid_epilogue(&mut code);

        for branch_offset in error_branches {
            let word = match branch_offset.kind {
                HybridBranchKind::TestErrorFlag => patch_test_bit_branch_imm14(
                    AARCH64_TBNZ_X0_63,
                    branch_offset.offset,
                    epilogue_offset,
                )?,
                HybridBranchKind::Uncond => {
                    patch_uncond_branch_imm26(AARCH64_B, branch_offset.offset, epilogue_offset)?
                }
            };
            code[branch_offset.offset..branch_offset.offset + 4]
                .copy_from_slice(&word.to_le_bytes());
        }

        ExecutableMemory::new(&code)
    }

    #[cfg(not(all(target_arch = "aarch64", target_endian = "little")))]
    {
        let _ = ops;
        Err(StencilError::Unsupported {
            path: "$".to_owned(),
            reason: "stencil backend currently requires little-endian AArch64",
        })
    }
}

pub(super) fn generate_encode_code(
    ops: &[EncodeStencilOp],
) -> Result<ExecutableMemory, StencilError> {
    #[cfg(all(target_arch = "aarch64", target_endian = "little"))]
    {
        let mut code = Vec::with_capacity(ops.len() * 40 + 96);
        emit_encode_prologue(&mut code);

        let mut error_branches = Vec::new();
        for op in ops {
            emit_encode_op(&mut code, op, &mut error_branches, 20, 0)?;
        }

        push_u32(&mut code, mov_w0_immediate(STENCIL_OK)?);
        let epilogue_offset = code.len();
        emit_encode_epilogue(&mut code);

        for branch in error_branches {
            let word = match branch.kind {
                EncodeBranchKind::CondHi => {
                    patch_cond_branch_imm19(AARCH64_B_HI, branch.offset, epilogue_offset)?
                }
                EncodeBranchKind::CondNe => {
                    patch_cond_branch_imm19(AARCH64_B_NE, branch.offset, epilogue_offset)?
                }
                EncodeBranchKind::Uncond => {
                    patch_uncond_branch_imm26(AARCH64_B, branch.offset, epilogue_offset)?
                }
            };
            code[branch.offset..branch.offset + 4].copy_from_slice(&word.to_le_bytes());
        }

        ExecutableMemory::new(&code)
    }

    #[cfg(not(all(target_arch = "aarch64", target_endian = "little")))]
    {
        let _ = ops;
        Err(StencilError::Unsupported {
            path: "$".to_owned(),
            reason: "stencil backend currently requires little-endian AArch64",
        })
    }
}

#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
const AARCH64_RET: u32 = 0xD65F_03C0;
#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
const AARCH64_CMP_W9_1: u32 = 0x7100_053F;
#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
const AARCH64_B_HI: u32 = 0x5400_0008;
#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
const AARCH64_B_EQ: u32 = 0x5400_0000;
#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
const AARCH64_B_NE: u32 = 0x5400_0001;
#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
const AARCH64_B_LS: u32 = 0x5400_0009;
#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
const AARCH64_B: u32 = 0x1400_0000;
#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
const AARCH64_CBZ_X0: u32 = 0xB400_0000;
#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
const AARCH64_CBNZ_X0: u32 = 0xB500_0000;
#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
const AARCH64_CMP_W0_0: u32 = 0x7100_001F;
#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
const AARCH64_STP_X29_X30_PRE: u32 = 0xA9BF_7BFD;
#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
const AARCH64_MOV_X29_SP: u32 = 0x9100_03FD;
#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
const AARCH64_STP_X19_X20_PRE: u32 = 0xA9BF_53F3;
#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
const AARCH64_STP_X21_X22_PRE: u32 = 0xA9BF_5BF5;
#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
const AARCH64_STP_X23_X24_PRE: u32 = 0xA9BF_63F7;
#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
const AARCH64_STP_X25_X26_PRE: u32 = 0xA9BF_6BF9;
#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
const AARCH64_STP_X27_X28_PRE: u32 = 0xA9BF_73FB;
#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
const AARCH64_MOV_X19_X0: u32 = 0xAA00_03F3;
#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
const AARCH64_MOV_X20_X1: u32 = 0xAA01_03F4;
#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
const AARCH64_MOV_X21_X2: u32 = 0xAA02_03F5;
#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
const AARCH64_MOV_X22_X3: u32 = 0xAA03_03F6;
#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
const AARCH64_MOV_X23_0: u32 = 0xD280_0017;
#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
const AARCH64_MOV_X0_X19: u32 = 0xAA13_03E0;
#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
const AARCH64_MOV_X1_X20: u32 = 0xAA14_03E1;
#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
const AARCH64_MOV_X2_X21: u32 = 0xAA15_03E2;
#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
const AARCH64_MOV_X4_X23: u32 = 0xAA17_03E4;
#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
const AARCH64_MOV_X23_X0: u32 = 0xAA00_03F7;
#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
const AARCH64_MOV_X0_X23: u32 = 0xAA17_03E0;
#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
const AARCH64_BLR_X16: u32 = 0xD63F_0200;
#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
const AARCH64_TBNZ_X0_63: u32 = 0xB7F8_0000;
#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
const AARCH64_LDP_X27_X28_POST: u32 = 0xA8C1_73FB;
#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
const AARCH64_LDP_X25_X26_POST: u32 = 0xA8C1_6BF9;
#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
const AARCH64_LDP_X23_X24_POST: u32 = 0xA8C1_63F7;
#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
const AARCH64_LDP_X21_X22_POST: u32 = 0xA8C1_5BF5;
#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
const AARCH64_LDP_X19_X20_POST: u32 = 0xA8C1_53F3;
#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
const AARCH64_LDP_X29_X30_POST: u32 = 0xA8C1_7BFD;

#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
#[derive(Debug, Clone, Copy)]
pub(super) struct BranchFixup {
    offset: usize,
    failure_index: usize,
    kind: BranchKind,
}

#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
#[derive(Debug, Clone, Copy)]
pub(super) struct HybridBranchFixup {
    offset: usize,
    kind: HybridBranchKind,
}

#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
#[derive(Debug, Clone, Copy)]
pub(super) enum HybridBranchKind {
    TestErrorFlag,
    Uncond,
}

#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
#[derive(Debug, Clone, Copy)]
pub(super) enum BranchKind {
    CondHi,
    Uncond,
}

#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
#[derive(Debug, Clone, Copy)]
pub(super) struct EncodeBranchFixup {
    offset: usize,
    kind: EncodeBranchKind,
}

#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
#[derive(Debug, Clone, Copy)]
pub(super) enum EncodeBranchKind {
    CondHi,
    CondNe,
    Uncond,
}

#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
fn emit_op(
    code: &mut Vec<u8>,
    op: &StencilOp,
    branches: &mut Vec<BranchFixup>,
) -> Result<(), StencilError> {
    match op {
        StencilOp::Copy(op) => emit_copy_op(code, *op),
        StencilOp::Bool {
            input_offset,
            output_offset,
            failure_index,
        } => emit_bool_op(
            code,
            *input_offset,
            *output_offset,
            *failure_index,
            branches,
        ),
        StencilOp::RootEnum {
            input_offset,
            cases,
            bodies,
            unknown_failure_index,
        } => emit_root_enum_op(
            code,
            *input_offset,
            cases,
            bodies,
            *unknown_failure_index,
            branches,
        ),
    }
}

#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
fn emit_copy_op(code: &mut Vec<u8>, op: CopyOp) -> Result<(), StencilError> {
    emit_copy_op_with_bases(code, op, 0, 2)
}

#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
fn emit_copy_op_with_bases(
    code: &mut Vec<u8>,
    op: CopyOp,
    input_base_reg: u8,
    output_base_reg: u8,
) -> Result<(), StencilError> {
    let (load, store) = match op.width {
        CopyWidth::One => (0x3840_0009, 0x3800_0049),
        CopyWidth::Two => (0x7840_0009, 0x7800_0049),
        CopyWidth::Four => (0xB840_0009, 0xB800_0049),
        CopyWidth::Eight => (0xF840_0009, 0xF800_0049),
    };
    push_u32(code, patch_ldur_stur_imm9(load, op.input_offset, "$input")?);
    let last = code.len() - 4;
    patch_registers(&mut code[last..last + 4], 9, input_base_reg)?;
    push_u32(
        code,
        patch_ldur_stur_imm9(store, op.output_offset, "$output")?,
    );
    let last = code.len() - 4;
    patch_registers(&mut code[last..last + 4], 9, output_base_reg)?;
    Ok(())
}

#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
fn emit_bool_op(
    code: &mut Vec<u8>,
    input_offset: usize,
    output_offset: Option<usize>,
    failure_index: usize,
    branches: &mut Vec<BranchFixup>,
) -> Result<(), StencilError> {
    push_u32(
        code,
        patch_ldur_stur_imm9(AARCH64_LDURB_W9_X0, input_offset, "$input")?,
    );
    push_u32(code, AARCH64_CMP_W9_1);
    let branch_offset = code.len();
    push_u32(code, 0);
    branches.push(BranchFixup {
        offset: branch_offset,
        failure_index,
        kind: BranchKind::CondHi,
    });
    if let Some(output_offset) = output_offset {
        push_u32(
            code,
            patch_ldur_stur_imm9(AARCH64_STURB_W9_X2, output_offset, "$output")?,
        );
    }
    Ok(())
}

#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
const AARCH64_LDURB_W9_X0: u32 = 0x3840_0009;
#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
const AARCH64_LDUR_W9_X0: u32 = 0xB840_0009;
#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
const AARCH64_STURB_W9_X2: u32 = 0x3800_0049;
#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
const AARCH64_STURB_W10_X2: u32 = 0x3800_004A;

#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
fn emit_root_enum_op(
    code: &mut Vec<u8>,
    input_offset: usize,
    cases: &[EnumCase],
    bodies: &[Vec<StencilOp>],
    unknown_failure_index: usize,
    branches: &mut Vec<BranchFixup>,
) -> Result<(), StencilError> {
    push_u32(
        code,
        patch_ldur_stur_imm9(AARCH64_LDUR_W9_X0, input_offset, "$input")?,
    );

    let mut case_branches = Vec::with_capacity(cases.len());
    for (case_index, case) in cases.iter().enumerate() {
        push_u32(code, cmp_w9_immediate(case.writer_index)?);
        let offset = code.len();
        push_u32(code, 0);
        case_branches.push((offset, case_index));
    }

    let unknown_branch = code.len();
    push_u32(code, 0);
    branches.push(BranchFixup {
        offset: unknown_branch,
        failure_index: unknown_failure_index,
        kind: BranchKind::Uncond,
    });

    let mut case_offsets = Vec::with_capacity(cases.len());
    let mut done_branches = Vec::new();
    for case in cases {
        case_offsets.push(code.len());
        if let Some(failure_index) = case.failure_index {
            let offset = code.len();
            push_u32(code, 0);
            branches.push(BranchFixup {
                offset,
                failure_index,
                kind: BranchKind::Uncond,
            });
            continue;
        }

        let Some(reader_discriminant) = case.reader_discriminant else {
            return Err(StencilError::Unsupported {
                path: "$code".to_owned(),
                reason: "readable enum case is missing reader discriminant",
            });
        };
        push_u32(code, mov_w10_immediate(u32::from(reader_discriminant))?);
        push_u32(
            code,
            patch_ldur_stur_imm9(AARCH64_STURB_W10_X2, 0, "$output")?,
        );

        let Some(body_index) = case.body_index else {
            return Err(StencilError::Unsupported {
                path: "$code".to_owned(),
                reason: "readable enum case is missing body ops",
            });
        };
        let Some(body) = bodies.get(body_index) else {
            return Err(StencilError::Unsupported {
                path: "$code".to_owned(),
                reason: "enum body index is out of range",
            });
        };
        for op in body {
            emit_op(code, op, branches)?;
        }
        let offset = code.len();
        push_u32(code, 0);
        done_branches.push(offset);
    }

    let done = code.len();
    for (offset, case_index) in case_branches {
        let Some(target) = case_offsets.get(case_index).copied() else {
            return Err(StencilError::Unsupported {
                path: "$code".to_owned(),
                reason: "enum case branch target is missing",
            });
        };
        let word = patch_cond_branch_imm19(AARCH64_B_EQ, offset, target)?;
        code[offset..offset + 4].copy_from_slice(&word.to_le_bytes());
    }
    for offset in done_branches {
        let word = patch_uncond_branch_imm26(AARCH64_B, offset, done)?;
        code[offset..offset + 4].copy_from_slice(&word.to_le_bytes());
    }

    Ok(())
}

#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
fn emit_hybrid_prologue(code: &mut Vec<u8>) {
    push_u32(code, AARCH64_STP_X29_X30_PRE);
    push_u32(code, AARCH64_MOV_X29_SP);
    push_u32(code, AARCH64_STP_X19_X20_PRE);
    push_u32(code, AARCH64_STP_X21_X22_PRE);
    push_u32(code, AARCH64_STP_X23_X24_PRE);
    push_u32(code, AARCH64_STP_X25_X26_PRE);
    push_u32(code, AARCH64_STP_X27_X28_PRE);
    push_u32(code, AARCH64_MOV_X19_X0);
    push_u32(code, AARCH64_MOV_X20_X1);
    push_u32(code, AARCH64_MOV_X21_X2);
    push_u32(code, AARCH64_MOV_X22_X3);
    push_u32(code, AARCH64_MOV_X23_0);
}

#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
fn emit_hybrid_epilogue(code: &mut Vec<u8>) {
    push_u32(code, AARCH64_LDP_X27_X28_POST);
    push_u32(code, AARCH64_LDP_X25_X26_POST);
    push_u32(code, AARCH64_LDP_X23_X24_POST);
    push_u32(code, AARCH64_LDP_X21_X22_POST);
    push_u32(code, AARCH64_LDP_X19_X20_POST);
    push_u32(code, AARCH64_LDP_X29_X30_POST);
    push_u32(code, AARCH64_RET);
}

#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
fn emit_hybrid_op(
    code: &mut Vec<u8>,
    op: &HybridStencilOp,
    error_branches: &mut Vec<HybridBranchFixup>,
) -> Result<(), StencilError> {
    emit_hybrid_op_with_output_base(code, op, error_branches, 22)
}

#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
fn emit_hybrid_op_with_output_base(
    code: &mut Vec<u8>,
    op: &HybridStencilOp,
    error_branches: &mut Vec<HybridBranchFixup>,
    output_base_register: u8,
) -> Result<(), StencilError> {
    match op {
        HybridStencilOp::Helper { helper_index } => {
            push_u32(code, AARCH64_MOV_X0_X19);
            push_u32(code, AARCH64_MOV_X1_X20);
            push_u32(code, AARCH64_MOV_X2_X21);
            push_u32(code, mov_x_register(3, output_base_register)?);
            push_u32(code, AARCH64_MOV_X4_X23);
            let helper_index =
                u64::try_from(*helper_index).map_err(|_| StencilError::Unsupported {
                    path: "$code".to_owned(),
                    reason: "stencil helper index exceeds u64",
                })?;
            emit_mov_x_immediate(code, 5, helper_index)?;
            emit_mov_x_immediate(code, 16, stencil_decode_helper as *const () as usize as u64)?;
            push_u32(code, AARCH64_BLR_X16);
            let branch_offset = code.len();
            push_u32(code, 0);
            error_branches.push(HybridBranchFixup {
                offset: branch_offset,
                kind: HybridBranchKind::TestErrorFlag,
            });
            push_u32(code, AARCH64_MOV_X23_X0);
        }
        HybridStencilOp::Copy {
            ops,
            input_len,
            failure_index,
        } => emit_hybrid_copy_op(
            code,
            ops,
            *input_len,
            *failure_index,
            error_branches,
            output_base_register,
        )?,
        HybridStencilOp::List {
            shape,
            output_offset,
            element_ops,
            element_stride,
            failure_index,
        } => {
            let list = HybridListEmit {
                shape,
                output_offset: *output_offset,
                element_ops,
                element_stride: *element_stride,
                failure_index: *failure_index,
            };
            emit_hybrid_list_op(code, list, error_branches)?;
        }
    }
    Ok(())
}

#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
fn emit_hybrid_copy_op(
    code: &mut Vec<u8>,
    ops: &[CopyOp],
    input_len: usize,
    _failure_index: usize,
    error_branches: &mut Vec<HybridBranchFixup>,
    output_base_register: u8,
) -> Result<(), StencilError> {
    emit_hybrid_check_available(code, input_len, error_branches)?;
    push_u32(code, add_x_register(24, 20, 23)?);
    for op in ops {
        emit_copy_op_with_bases(code, *op, 24, output_base_register)?;
    }
    if input_len != 0 {
        push_u32(code, add_x_immediate(23, 23, input_len, "$cursor")?);
    }
    Ok(())
}

#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
struct HybridListEmit<'a> {
    shape: &'static Shape,
    output_offset: usize,
    element_ops: &'a [HybridStencilOp],
    element_stride: usize,
    failure_index: usize,
}

#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
fn emit_hybrid_list_op(
    code: &mut Vec<u8>,
    list: HybridListEmit<'_>,
    error_branches: &mut Vec<HybridBranchFixup>,
) -> Result<(), StencilError> {
    emit_hybrid_check_available(code, 4, error_branches)?;
    push_u32(code, add_x_register(24, 20, 23)?);
    push_u32(code, ldur_w_register(25, 24, 0, "$list")?);
    push_u32(code, add_x_immediate(23, 23, 4, "$list")?);

    if list.output_offset == 0 {
        push_u32(code, mov_x_register(0, 22)?);
    } else {
        push_u32(code, add_x_immediate(0, 22, list.output_offset, "$list")?);
    }
    emit_mov_x_immediate(code, 1, list.shape as *const Shape as usize as u64)?;
    push_u32(code, mov_x_register(2, 25)?);
    emit_mov_x_immediate(
        code,
        16,
        stencil_decode_list_begin as *const () as usize as u64,
    )?;
    push_u32(code, AARCH64_BLR_X16);

    let begin_succeeded_branch = code.len();
    push_u32(code, 0);
    emit_hybrid_failure_block(code, list.failure_index, error_branches)?;
    let begin_success = code.len();
    let begin_succeeded_word =
        patch_compare_zero_branch_imm19(AARCH64_CBNZ_X0, begin_succeeded_branch, begin_success)?;
    code[begin_succeeded_branch..begin_succeeded_branch + 4]
        .copy_from_slice(&begin_succeeded_word.to_le_bytes());

    push_u32(code, mov_x_register(26, 0)?);
    push_u32(code, mov_x_register(27, 26)?);
    emit_mov_x_immediate(code, 28, 0)?;

    let loop_check = code.len();
    push_u32(code, cmp_x_register(28, 25)?);
    let done_branch = code.len();
    push_u32(code, 0);

    for op in list.element_ops {
        emit_hybrid_op_with_output_base(code, op, error_branches, 27)?;
    }
    if list.element_stride != 0 {
        push_u32(code, add_x_immediate(27, 27, list.element_stride, "$list")?);
    }
    push_u32(code, add_x_immediate(28, 28, 1, "$list")?);
    let continue_branch = code.len();
    push_u32(code, 0);

    let finish_offset = code.len();
    let done_word = patch_cond_branch_imm19(AARCH64_B_EQ, done_branch, finish_offset)?;
    code[done_branch..done_branch + 4].copy_from_slice(&done_word.to_le_bytes());
    let continue_word = patch_uncond_branch_imm26(AARCH64_B, continue_branch, loop_check)?;
    code[continue_branch..continue_branch + 4].copy_from_slice(&continue_word.to_le_bytes());

    if list.output_offset == 0 {
        push_u32(code, mov_x_register(0, 22)?);
    } else {
        push_u32(code, add_x_immediate(0, 22, list.output_offset, "$list")?);
    }
    emit_mov_x_immediate(code, 1, list.shape as *const Shape as usize as u64)?;
    push_u32(code, mov_x_register(2, 25)?);
    emit_mov_x_immediate(
        code,
        16,
        stencil_decode_list_finish as *const () as usize as u64,
    )?;
    push_u32(code, AARCH64_BLR_X16);
    push_u32(code, AARCH64_CMP_W0_0);
    let finish_succeeded_branch = code.len();
    push_u32(code, 0);
    emit_hybrid_failure_block(code, list.failure_index, error_branches)?;
    let finish_success = code.len();
    let finish_succeeded_word =
        patch_cond_branch_imm19(AARCH64_B_EQ, finish_succeeded_branch, finish_success)?;
    code[finish_succeeded_branch..finish_succeeded_branch + 4]
        .copy_from_slice(&finish_succeeded_word.to_le_bytes());
    Ok(())
}

#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
fn emit_hybrid_check_available(
    code: &mut Vec<u8>,
    len: usize,
    error_branches: &mut Vec<HybridBranchFixup>,
) -> Result<(), StencilError> {
    if len == 0 {
        return Ok(());
    }
    push_u32(code, add_x_immediate(10, 23, len, "$cursor")?);
    push_u32(code, cmp_x_register(10, 21)?);
    let success_branch = code.len();
    push_u32(code, 0);
    push_u32(code, mov_x_register(0, 10)?);
    let failure_branch = code.len();
    push_u32(code, 0);
    error_branches.push(HybridBranchFixup {
        offset: failure_branch,
        kind: HybridBranchKind::Uncond,
    });
    let success = code.len();
    let success_word = patch_cond_branch_imm19(AARCH64_B_LS, success_branch, success)?;
    code[success_branch..success_branch + 4].copy_from_slice(&success_word.to_le_bytes());
    Ok(())
}

#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
fn emit_hybrid_failure_block(
    code: &mut Vec<u8>,
    failure_index: usize,
    error_branches: &mut Vec<HybridBranchFixup>,
) -> Result<(), StencilError> {
    let status = HYBRID_ERROR_FLAG as u64 | u64::from(status_for_failure(failure_index)?);
    emit_mov_x_immediate(code, 0, status)?;
    let offset = code.len();
    push_u32(code, 0);
    error_branches.push(HybridBranchFixup {
        offset,
        kind: HybridBranchKind::Uncond,
    });
    Ok(())
}

#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
fn emit_encode_prologue(code: &mut Vec<u8>) {
    push_u32(code, AARCH64_STP_X29_X30_PRE);
    push_u32(code, AARCH64_MOV_X29_SP);
    push_u32(code, AARCH64_STP_X19_X20_PRE);
    push_u32(code, AARCH64_STP_X21_X22_PRE);
    push_u32(code, AARCH64_STP_X23_X24_PRE);
    push_u32(code, AARCH64_STP_X25_X26_PRE);
    push_u32(code, AARCH64_STP_X27_X28_PRE);
    push_u32(code, AARCH64_MOV_X19_X0);
    push_u32(code, AARCH64_MOV_X20_X1);
    push_u32(code, AARCH64_MOV_X21_X2);
}

#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
fn emit_encode_epilogue(code: &mut Vec<u8>) {
    push_u32(code, AARCH64_LDP_X27_X28_POST);
    push_u32(code, AARCH64_LDP_X25_X26_POST);
    push_u32(code, AARCH64_LDP_X23_X24_POST);
    push_u32(code, AARCH64_LDP_X21_X22_POST);
    push_u32(code, AARCH64_LDP_X19_X20_POST);
    push_u32(code, AARCH64_LDP_X29_X30_POST);
    push_u32(code, AARCH64_RET);
}

#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
fn emit_direct_encode_prologue(code: &mut Vec<u8>) {
    push_u32(code, AARCH64_STP_X29_X30_PRE);
    push_u32(code, AARCH64_MOV_X29_SP);
    push_u32(code, AARCH64_STP_X19_X20_PRE);
    push_u32(code, AARCH64_STP_X21_X22_PRE);
    push_u32(code, AARCH64_MOV_X19_X0);
    push_u32(code, AARCH64_MOV_X20_X1);
}

#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
fn emit_direct_encode_epilogue(code: &mut Vec<u8>) {
    push_u32(code, AARCH64_LDP_X21_X22_POST);
    push_u32(code, AARCH64_LDP_X19_X20_POST);
    push_u32(code, AARCH64_LDP_X29_X30_POST);
    push_u32(code, AARCH64_RET);
}

#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
fn emit_encode_op(
    code: &mut Vec<u8>,
    op: &EncodeStencilOp,
    error_branches: &mut Vec<EncodeBranchFixup>,
    value_base_reg: u8,
    option_depth: usize,
) -> Result<(), StencilError> {
    match op {
        EncodeStencilOp::Helper { helper_index } => {
            push_u32(code, AARCH64_MOV_X0_X19);
            push_u32(code, mov_x_register(1, value_base_reg)?);
            push_u32(code, AARCH64_MOV_X2_X21);
            let helper_index =
                u64::try_from(*helper_index).map_err(|_| StencilError::Unsupported {
                    path: "$code".to_owned(),
                    reason: "stencil helper index exceeds u64",
                })?;
            emit_mov_x_immediate(code, 3, helper_index)?;
            emit_mov_x_immediate(code, 16, stencil_encode_helper as *const () as usize as u64)?;
            push_u32(code, AARCH64_BLR_X16);
            push_u32(code, AARCH64_CMP_W0_0);
            let branch_offset = code.len();
            push_u32(code, 0);
            error_branches.push(EncodeBranchFixup {
                offset: branch_offset,
                kind: EncodeBranchKind::CondNe,
            });
        }
        EncodeStencilOp::Direct { ops, output_len } => {
            push_u32(code, mov_x_register(0, 21)?);
            let output_len = u64::try_from(*output_len).map_err(|_| StencilError::Unsupported {
                path: "$code".to_owned(),
                reason: "direct encode output length exceeds u64",
            })?;
            emit_mov_x_immediate(code, 1, output_len)?;
            emit_mov_x_immediate(
                code,
                16,
                stencil_encode_reserve as *const () as usize as u64,
            )?;
            push_u32(code, AARCH64_BLR_X16);

            let reserve_succeeded_branch = code.len();
            push_u32(code, 0);

            push_u32(code, mov_w0_immediate(1)?);
            let reserve_failed_branch = code.len();
            push_u32(code, 0);
            error_branches.push(EncodeBranchFixup {
                offset: reserve_failed_branch,
                kind: EncodeBranchKind::Uncond,
            });

            let copy_start = code.len();
            let branch_word = patch_compare_zero_branch_imm19(
                AARCH64_CBNZ_X0,
                reserve_succeeded_branch,
                copy_start,
            )?;
            code[reserve_succeeded_branch..reserve_succeeded_branch + 4]
                .copy_from_slice(&branch_word.to_le_bytes());

            push_u32(code, mov_x_register(2, 0)?);
            push_u32(code, mov_x_register(0, value_base_reg)?);
            for op in ops {
                emit_copy_op(code, *op)?;
            }
        }
        EncodeStencilOp::Bytes {
            shape,
            input_offset,
            kind,
        } => emit_encode_bytes_op(
            code,
            shape,
            *input_offset,
            *kind,
            error_branches,
            value_base_reg,
        )?,
        EncodeStencilOp::Enum {
            shape,
            input_offset,
            cases,
        } => emit_encode_enum_op(
            code,
            shape,
            *input_offset,
            cases,
            error_branches,
            value_base_reg,
            option_depth,
        )?,
        EncodeStencilOp::Option {
            shape,
            input_offset,
            layout,
            some_ops,
        } => emit_encode_option_op(
            code,
            EncodeOptionEmit {
                shape,
                input_offset: *input_offset,
                layout: *layout,
                some_ops,
                value_base_reg,
                option_depth,
            },
            error_branches,
        )?,
        EncodeStencilOp::List {
            shape,
            input_offset,
            layout,
            element_ops,
        } => emit_encode_list_op(
            code,
            shape,
            *input_offset,
            *layout,
            element_ops,
            error_branches,
            value_base_reg,
        )?,
    }
    Ok(())
}

#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
fn emit_encode_bytes_op(
    code: &mut Vec<u8>,
    shape: &'static Shape,
    input_offset: usize,
    kind: EncodeBytesKind,
    error_branches: &mut Vec<EncodeBranchFixup>,
    value_base_reg: u8,
) -> Result<(), StencilError> {
    if kind == EncodeBytesKind::String {
        let Some(layout) = string_layout() else {
            return Err(StencilError::Unsupported {
                path: "$string".to_owned(),
                reason: "string layout probe failed",
            });
        };
        if input_offset == 0 {
            push_u32(code, mov_x_register(10, value_base_reg)?);
        } else {
            push_u32(
                code,
                add_x_immediate(10, value_base_reg, input_offset, "$input")?,
            );
        }
        push_u32(code, ldur_x_register(22, 10, layout.ptr_offset, "$string")?);
        push_u32(code, ldur_x_register(23, 10, layout.len_offset, "$string")?);
    } else {
        if input_offset == 0 {
            push_u32(code, mov_x_register(0, value_base_reg)?);
        } else {
            push_u32(
                code,
                add_x_immediate(0, value_base_reg, input_offset, "$input")?,
            );
        }
        emit_mov_x_immediate(code, 1, shape as *const Shape as usize as u64)?;
        let kind = u64::try_from(kind.abi_tag()).map_err(|_| StencilError::Unsupported {
            path: "$code".to_owned(),
            reason: "stencil byte kind exceeds u64",
        })?;
        emit_mov_x_immediate(code, 2, kind)?;
        emit_mov_x_immediate(
            code,
            16,
            stencil_encode_byte_parts as *const () as usize as u64,
        )?;
        push_u32(code, AARCH64_BLR_X16);

        let parts_succeeded_branch = code.len();
        push_u32(code, 0);
        emit_encode_failure_branch(code, error_branches)?;

        let parts_success = code.len();
        let parts_branch_word = patch_compare_zero_branch_imm19(
            AARCH64_CBNZ_X0,
            parts_succeeded_branch,
            parts_success,
        )?;
        code[parts_succeeded_branch..parts_succeeded_branch + 4]
            .copy_from_slice(&parts_branch_word.to_le_bytes());

        push_u32(code, mov_x_register(22, 0)?);
        push_u32(code, mov_x_register(23, 1)?);
    }

    emit_mov_x_immediate(code, 10, u64::from(u32::MAX))?;
    push_u32(code, cmp_x_register(23, 10)?);
    let len_succeeded_branch = code.len();
    push_u32(code, 0);
    emit_encode_failure_branch(code, error_branches)?;
    let len_success = code.len();
    let len_branch_word = patch_cond_branch_imm19(AARCH64_B_LS, len_succeeded_branch, len_success)?;
    code[len_succeeded_branch..len_succeeded_branch + 4]
        .copy_from_slice(&len_branch_word.to_le_bytes());

    push_u32(code, mov_x_register(0, 21)?);
    push_u32(code, add_x_immediate(1, 23, 4, "$bytes")?);
    emit_mov_x_immediate(
        code,
        16,
        stencil_encode_reserve as *const () as usize as u64,
    )?;
    push_u32(code, AARCH64_BLR_X16);

    let reserve_succeeded_branch = code.len();
    push_u32(code, 0);
    emit_encode_failure_branch(code, error_branches)?;

    let reserve_success = code.len();
    let reserve_branch_word = patch_compare_zero_branch_imm19(
        AARCH64_CBNZ_X0,
        reserve_succeeded_branch,
        reserve_success,
    )?;
    code[reserve_succeeded_branch..reserve_succeeded_branch + 4]
        .copy_from_slice(&reserve_branch_word.to_le_bytes());

    push_u32(code, stur_w_register(23, 0, 0, "$bytes")?);
    push_u32(code, add_x_immediate(0, 0, 4, "$bytes")?);
    push_u32(code, mov_x_register(1, 22)?);
    push_u32(code, mov_x_register(2, 23)?);
    emit_mov_x_immediate(code, 16, stencil_copy_bytes as *const () as usize as u64)?;
    push_u32(code, AARCH64_BLR_X16);

    Ok(())
}

#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
fn emit_encode_failure_branch(
    code: &mut Vec<u8>,
    error_branches: &mut Vec<EncodeBranchFixup>,
) -> Result<(), StencilError> {
    push_u32(code, mov_w0_immediate(1)?);
    let reserve_failed_branch = code.len();
    push_u32(code, 0);
    error_branches.push(EncodeBranchFixup {
        offset: reserve_failed_branch,
        kind: EncodeBranchKind::Uncond,
    });
    Ok(())
}

#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
fn emit_encode_enum_op(
    code: &mut Vec<u8>,
    shape: &'static Shape,
    input_offset: usize,
    cases: &[EncodeEnumCase],
    error_branches: &mut Vec<EncodeBranchFixup>,
    value_base_reg: u8,
    option_depth: usize,
) -> Result<(), StencilError> {
    if input_offset == 0 {
        push_u32(code, mov_x_register(0, value_base_reg)?);
    } else {
        push_u32(
            code,
            add_x_immediate(0, value_base_reg, input_offset, "$input")?,
        );
    }
    emit_mov_x_immediate(code, 1, shape as *const Shape as usize as u64)?;
    emit_mov_x_immediate(
        code,
        16,
        stencil_enum_variant_index as *const () as usize as u64,
    )?;
    push_u32(code, AARCH64_BLR_X16);
    push_u32(code, mov_x_register(24, 0)?);

    let mut case_branches = Vec::with_capacity(cases.len());
    for (case_index, case) in cases.iter().enumerate() {
        push_u32(code, cmp_x_immediate(24, case.facet_index, "$enum")?);
        let offset = code.len();
        push_u32(code, 0);
        case_branches.push((offset, case_index));
    }

    emit_encode_failure_branch(code, error_branches)?;

    let mut case_offsets = Vec::with_capacity(cases.len());
    let mut done_branches = Vec::new();
    for case in cases {
        case_offsets.push(code.len());
        emit_encode_wire_index(code, case.wire_index, error_branches)?;
        for op in &case.ops {
            emit_encode_op(code, op, error_branches, value_base_reg, option_depth)?;
        }
        let done_branch = code.len();
        push_u32(code, 0);
        done_branches.push(done_branch);
    }

    let done = code.len();
    for (offset, case_index) in case_branches {
        let Some(target) = case_offsets.get(case_index).copied() else {
            return Err(StencilError::Unsupported {
                path: "$code".to_owned(),
                reason: "enum encode case branch target is missing",
            });
        };
        let word = patch_cond_branch_imm19(AARCH64_B_EQ, offset, target)?;
        code[offset..offset + 4].copy_from_slice(&word.to_le_bytes());
    }
    for offset in done_branches {
        let word = patch_uncond_branch_imm26(AARCH64_B, offset, done)?;
        code[offset..offset + 4].copy_from_slice(&word.to_le_bytes());
    }

    Ok(())
}

#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
struct EncodeOptionEmit<'a> {
    shape: &'static Shape,
    input_offset: usize,
    layout: EncodeOptionLayout,
    some_ops: &'a [EncodeStencilOp],
    value_base_reg: u8,
    option_depth: usize,
}

#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
fn emit_encode_option_op(
    code: &mut Vec<u8>,
    option: EncodeOptionEmit<'_>,
    error_branches: &mut Vec<EncodeBranchFixup>,
) -> Result<(), StencilError> {
    if option.layout == EncodeOptionLayout::NicheString {
        return emit_encode_niche_string_option_op(code, option, error_branches);
    }

    let some_base_reg = option_value_base_register(option.option_depth)?;
    if option.input_offset == 0 {
        push_u32(code, mov_x_register(0, option.value_base_reg)?);
    } else {
        push_u32(
            code,
            add_x_immediate(0, option.value_base_reg, option.input_offset, "$input")?,
        );
    }
    emit_mov_x_immediate(code, 1, option.shape as *const Shape as usize as u64)?;
    emit_mov_x_immediate(code, 16, stencil_option_parts as *const () as usize as u64)?;
    push_u32(code, AARCH64_BLR_X16);
    push_u32(code, mov_x_register(24, 1)?);

    push_u32(code, cmp_x_immediate(0, STENCIL_OPTION_NONE, "$option")?);
    let none_branch = code.len();
    push_u32(code, 0);
    push_u32(code, cmp_x_immediate(0, STENCIL_OPTION_SOME, "$option")?);
    let some_branch = code.len();
    push_u32(code, 0);
    emit_encode_failure_branch(code, error_branches)?;

    let none_offset = code.len();
    emit_encode_tag_byte(code, STENCIL_OPTION_NONE, error_branches)?;
    let none_done_branch = code.len();
    push_u32(code, 0);

    let some_offset = code.len();
    emit_encode_tag_byte(code, STENCIL_OPTION_SOME, error_branches)?;
    push_u32(code, mov_x_register(some_base_reg, 24)?);
    for op in option.some_ops {
        emit_encode_op(
            code,
            op,
            error_branches,
            some_base_reg,
            option.option_depth + 1,
        )?;
    }
    let some_done_branch = code.len();
    push_u32(code, 0);

    let done = code.len();
    let none_word = patch_cond_branch_imm19(AARCH64_B_EQ, none_branch, none_offset)?;
    code[none_branch..none_branch + 4].copy_from_slice(&none_word.to_le_bytes());
    let some_word = patch_cond_branch_imm19(AARCH64_B_EQ, some_branch, some_offset)?;
    code[some_branch..some_branch + 4].copy_from_slice(&some_word.to_le_bytes());
    let none_done_word = patch_uncond_branch_imm26(AARCH64_B, none_done_branch, done)?;
    code[none_done_branch..none_done_branch + 4].copy_from_slice(&none_done_word.to_le_bytes());
    let some_done_word = patch_uncond_branch_imm26(AARCH64_B, some_done_branch, done)?;
    code[some_done_branch..some_done_branch + 4].copy_from_slice(&some_done_word.to_le_bytes());
    Ok(())
}

#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
fn emit_encode_niche_string_option_op(
    code: &mut Vec<u8>,
    option: EncodeOptionEmit<'_>,
    error_branches: &mut Vec<EncodeBranchFixup>,
) -> Result<(), StencilError> {
    let Some(layout) = option_string_layout() else {
        return Err(StencilError::Unsupported {
            path: "$option".to_owned(),
            reason: "option string layout probe failed",
        });
    };
    if !layout.same_size_niche {
        return Err(StencilError::Unsupported {
            path: "$option".to_owned(),
            reason: "Option<String> is not represented as a niche string",
        });
    }

    let some_base_reg = option_value_base_register(option.option_depth)?;
    if option.input_offset == 0 {
        push_u32(code, mov_x_register(some_base_reg, option.value_base_reg)?);
    } else {
        push_u32(
            code,
            add_x_immediate(
                some_base_reg,
                option.value_base_reg,
                option.input_offset,
                "$option",
            )?,
        );
    }
    push_u32(
        code,
        ldur_x_register(24, some_base_reg, layout.none_tag_offset, "$option")?,
    );
    emit_mov_x_immediate(code, 10, layout.none_tag_value as u64)?;
    push_u32(code, cmp_x_register(24, 10)?);
    let none_branch = code.len();
    push_u32(code, 0);

    emit_encode_tag_byte(code, STENCIL_OPTION_SOME, error_branches)?;
    for op in option.some_ops {
        emit_encode_op(
            code,
            op,
            error_branches,
            some_base_reg,
            option.option_depth + 1,
        )?;
    }
    let some_done_branch = code.len();
    push_u32(code, 0);

    let none_offset = code.len();
    emit_encode_tag_byte(code, STENCIL_OPTION_NONE, error_branches)?;

    let done = code.len();
    let none_word = patch_cond_branch_imm19(AARCH64_B_EQ, none_branch, none_offset)?;
    code[none_branch..none_branch + 4].copy_from_slice(&none_word.to_le_bytes());
    let some_done_word = patch_uncond_branch_imm26(AARCH64_B, some_done_branch, done)?;
    code[some_done_branch..some_done_branch + 4].copy_from_slice(&some_done_word.to_le_bytes());
    Ok(())
}

#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
fn emit_encode_list_op(
    code: &mut Vec<u8>,
    shape: &'static Shape,
    input_offset: usize,
    layout: EncodeListLayout,
    element_ops: &[EncodeStencilOp],
    error_branches: &mut Vec<EncodeBranchFixup>,
    value_base_reg: u8,
) -> Result<(), StencilError> {
    if let EncodeListLayout::Vec {
        ptr_offset,
        len_offset,
        element_stride,
    } = layout
    {
        return emit_encode_vec_list_op(
            code,
            EncodeVecListEmit {
                input_offset,
                ptr_offset,
                len_offset,
                element_stride,
                element_ops,
                value_base_reg,
            },
            error_branches,
        );
    }

    if input_offset == 0 {
        push_u32(code, mov_x_register(0, value_base_reg)?);
    } else {
        push_u32(
            code,
            add_x_immediate(0, value_base_reg, input_offset, "$list")?,
        );
    }
    push_u32(code, mov_x_register(26, 0)?);
    emit_mov_x_immediate(code, 1, shape as *const Shape as usize as u64)?;
    emit_mov_x_immediate(code, 16, stencil_list_len as *const () as usize as u64)?;
    push_u32(code, AARCH64_BLR_X16);
    push_u32(code, mov_x_register(27, 0)?);

    emit_mov_x_immediate(code, 10, u32::MAX as u64)?;
    push_u32(code, cmp_x_register(27, 10)?);
    let len_failed_branch = code.len();
    push_u32(code, 0);
    error_branches.push(EncodeBranchFixup {
        offset: len_failed_branch,
        kind: EncodeBranchKind::CondHi,
    });

    emit_encode_u32_register(code, 27, error_branches)?;
    emit_mov_x_immediate(code, 28, 0)?;

    let loop_check = code.len();
    push_u32(code, cmp_x_register(28, 27)?);
    let done_branch = code.len();
    push_u32(code, 0);

    push_u32(code, mov_x_register(0, 26)?);
    emit_mov_x_immediate(code, 1, shape as *const Shape as usize as u64)?;
    push_u32(code, mov_x_register(2, 28)?);
    emit_mov_x_immediate(code, 16, stencil_list_element as *const () as usize as u64)?;
    push_u32(code, AARCH64_BLR_X16);

    let element_succeeded_branch = code.len();
    push_u32(code, 0);
    emit_encode_failure_branch(code, error_branches)?;
    let element_success = code.len();
    let element_branch_word = patch_compare_zero_branch_imm19(
        AARCH64_CBNZ_X0,
        element_succeeded_branch,
        element_success,
    )?;
    code[element_succeeded_branch..element_succeeded_branch + 4]
        .copy_from_slice(&element_branch_word.to_le_bytes());

    push_u32(code, mov_x_register(25, 0)?);
    emit_push_list_state(code);
    for op in element_ops {
        emit_encode_op(code, op, error_branches, 25, 1)?;
    }
    emit_pop_list_state(code);

    push_u32(code, add_x_immediate(28, 28, 1, "$list")?);
    let continue_branch = code.len();
    push_u32(code, 0);

    let done = code.len();
    let done_word = patch_cond_branch_imm19(AARCH64_B_EQ, done_branch, done)?;
    code[done_branch..done_branch + 4].copy_from_slice(&done_word.to_le_bytes());
    let continue_word = patch_uncond_branch_imm26(AARCH64_B, continue_branch, loop_check)?;
    code[continue_branch..continue_branch + 4].copy_from_slice(&continue_word.to_le_bytes());
    restore_list_parent_base(code, value_base_reg, input_offset, "$list")?;
    Ok(())
}

#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
struct EncodeVecListEmit<'a> {
    input_offset: usize,
    ptr_offset: usize,
    len_offset: usize,
    element_stride: usize,
    element_ops: &'a [EncodeStencilOp],
    value_base_reg: u8,
}

#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
fn emit_encode_vec_list_op(
    code: &mut Vec<u8>,
    list: EncodeVecListEmit<'_>,
    error_branches: &mut Vec<EncodeBranchFixup>,
) -> Result<(), StencilError> {
    if list.input_offset == 0 {
        push_u32(code, mov_x_register(26, list.value_base_reg)?);
    } else {
        push_u32(
            code,
            add_x_immediate(26, list.value_base_reg, list.input_offset, "$list")?,
        );
    }
    push_u32(code, ldur_x_register(25, 26, list.ptr_offset, "$list")?);
    push_u32(code, ldur_x_register(27, 26, list.len_offset, "$list")?);

    emit_mov_x_immediate(code, 10, u32::MAX as u64)?;
    push_u32(code, cmp_x_register(27, 10)?);
    let len_failed_branch = code.len();
    push_u32(code, 0);
    error_branches.push(EncodeBranchFixup {
        offset: len_failed_branch,
        kind: EncodeBranchKind::CondHi,
    });

    emit_encode_u32_register(code, 27, error_branches)?;
    emit_mov_x_immediate(code, 28, 0)?;

    let loop_check = code.len();
    push_u32(code, cmp_x_register(28, 27)?);
    let done_branch = code.len();
    push_u32(code, 0);

    emit_push_list_state(code);
    for op in list.element_ops {
        emit_encode_op(code, op, error_branches, 25, 1)?;
    }
    emit_pop_list_state(code);

    if list.element_stride != 0 {
        push_u32(code, add_x_immediate(25, 25, list.element_stride, "$list")?);
    }
    push_u32(code, add_x_immediate(28, 28, 1, "$list")?);
    let continue_branch = code.len();
    push_u32(code, 0);

    let done = code.len();
    let done_word = patch_cond_branch_imm19(AARCH64_B_EQ, done_branch, done)?;
    code[done_branch..done_branch + 4].copy_from_slice(&done_word.to_le_bytes());
    let continue_word = patch_uncond_branch_imm26(AARCH64_B, continue_branch, loop_check)?;
    code[continue_branch..continue_branch + 4].copy_from_slice(&continue_word.to_le_bytes());
    restore_list_parent_base(code, list.value_base_reg, list.input_offset, "$list")?;
    Ok(())
}

#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
fn restore_list_parent_base(
    code: &mut Vec<u8>,
    value_base_reg: u8,
    input_offset: usize,
    path: &str,
) -> Result<(), StencilError> {
    if !(25..=28).contains(&value_base_reg) {
        return Ok(());
    }
    if input_offset == 0 {
        push_u32(code, mov_x_register(value_base_reg, 26)?);
    } else {
        push_u32(
            code,
            sub_x_immediate(value_base_reg, 26, input_offset, path)?,
        );
    }
    Ok(())
}

#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
fn emit_encode_wire_index(
    code: &mut Vec<u8>,
    wire_index: u32,
    error_branches: &mut Vec<EncodeBranchFixup>,
) -> Result<(), StencilError> {
    push_u32(code, mov_x_register(0, 21)?);
    emit_mov_x_immediate(code, 1, 4)?;
    emit_mov_x_immediate(
        code,
        16,
        stencil_encode_reserve as *const () as usize as u64,
    )?;
    push_u32(code, AARCH64_BLR_X16);

    let reserve_succeeded_branch = code.len();
    push_u32(code, 0);
    emit_encode_failure_branch(code, error_branches)?;

    let reserve_success = code.len();
    let reserve_branch_word = patch_compare_zero_branch_imm19(
        AARCH64_CBNZ_X0,
        reserve_succeeded_branch,
        reserve_success,
    )?;
    code[reserve_succeeded_branch..reserve_succeeded_branch + 4]
        .copy_from_slice(&reserve_branch_word.to_le_bytes());

    emit_mov_x_immediate(code, 10, u64::from(wire_index))?;
    push_u32(code, stur_w_register(10, 0, 0, "$enum")?);
    Ok(())
}

#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
fn emit_encode_u32_register(
    code: &mut Vec<u8>,
    register: u8,
    error_branches: &mut Vec<EncodeBranchFixup>,
) -> Result<(), StencilError> {
    push_u32(code, mov_x_register(0, 21)?);
    emit_mov_x_immediate(code, 1, 4)?;
    emit_mov_x_immediate(
        code,
        16,
        stencil_encode_reserve as *const () as usize as u64,
    )?;
    push_u32(code, AARCH64_BLR_X16);

    let reserve_succeeded_branch = code.len();
    push_u32(code, 0);
    emit_encode_failure_branch(code, error_branches)?;

    let reserve_success = code.len();
    let reserve_branch_word = patch_compare_zero_branch_imm19(
        AARCH64_CBNZ_X0,
        reserve_succeeded_branch,
        reserve_success,
    )?;
    code[reserve_succeeded_branch..reserve_succeeded_branch + 4]
        .copy_from_slice(&reserve_branch_word.to_le_bytes());

    push_u32(code, stur_w_register(register, 0, 0, "$list")?);
    Ok(())
}

#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
fn emit_encode_tag_byte(
    code: &mut Vec<u8>,
    value: usize,
    error_branches: &mut Vec<EncodeBranchFixup>,
) -> Result<(), StencilError> {
    push_u32(code, mov_x_register(0, 21)?);
    emit_mov_x_immediate(code, 1, 1)?;
    emit_mov_x_immediate(
        code,
        16,
        stencil_encode_reserve as *const () as usize as u64,
    )?;
    push_u32(code, AARCH64_BLR_X16);

    let reserve_succeeded_branch = code.len();
    push_u32(code, 0);
    emit_encode_failure_branch(code, error_branches)?;

    let reserve_success = code.len();
    let reserve_branch_word = patch_compare_zero_branch_imm19(
        AARCH64_CBNZ_X0,
        reserve_succeeded_branch,
        reserve_success,
    )?;
    code[reserve_succeeded_branch..reserve_succeeded_branch + 4]
        .copy_from_slice(&reserve_branch_word.to_le_bytes());

    let value = u32::try_from(value).map_err(|_| StencilError::Unsupported {
        path: "$option".to_owned(),
        reason: "option tag exceeds u32",
    })?;
    push_u32(code, mov_w10_immediate(value)?);
    push_u32(code, stur_b_register(10, 0, 0, "$option")?);
    Ok(())
}

#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
fn option_value_base_register(option_depth: usize) -> Result<u8, StencilError> {
    let register = 25usize
        .checked_add(option_depth)
        .ok_or_else(|| StencilError::Unsupported {
            path: "$option".to_owned(),
            reason: "nested option stencil depth overflows",
        })?;
    if register > 28 {
        return Err(StencilError::Unsupported {
            path: "$option".to_owned(),
            reason: "nested option stencil depth exceeds reserved registers",
        });
    }
    Ok(register as u8)
}

#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
fn emit_push_list_state(code: &mut Vec<u8>) {
    push_u32(code, 0xD100_83FF);
    push_u32(code, 0xA900_6FFA);
    push_u32(code, 0xF900_0BFC);
    push_u32(code, 0xF900_0FF9);
}

#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
fn emit_pop_list_state(code: &mut Vec<u8>) {
    push_u32(code, 0xF940_0FF9);
    push_u32(code, 0xF940_0BFC);
    push_u32(code, 0xA940_6FFA);
    push_u32(code, 0x9100_83FF);
}

#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
fn patch_ldur_stur_imm9(base: u32, offset: usize, path: &str) -> Result<u32, StencilError> {
    let imm = i32::try_from(offset).map_err(|_| StencilError::Unsupported {
        path: path.to_owned(),
        reason: "stencil offset exceeds AArch64 imm9 range",
    })?;
    if !(-256..=255).contains(&imm) {
        return Err(StencilError::Unsupported {
            path: path.to_owned(),
            reason: "stencil offset exceeds AArch64 imm9 range",
        });
    }
    let imm9 = (imm as u32) & 0x1ff;
    Ok(base | (imm9 << 12))
}

#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
fn patch_cond_branch_imm19(
    base: u32,
    branch_offset: usize,
    target: usize,
) -> Result<u32, StencilError> {
    let branch_offset = isize::try_from(branch_offset).map_err(|_| StencilError::Unsupported {
        path: "$code".to_owned(),
        reason: "stencil branch offset exceeds isize",
    })?;
    let target = isize::try_from(target).map_err(|_| StencilError::Unsupported {
        path: "$code".to_owned(),
        reason: "stencil branch target exceeds isize",
    })?;
    let delta = target
        .checked_sub(branch_offset)
        .ok_or_else(|| StencilError::Unsupported {
            path: "$code".to_owned(),
            reason: "stencil branch offset overflow",
        })?;
    if delta % 4 != 0 {
        return Err(StencilError::Unsupported {
            path: "$code".to_owned(),
            reason: "stencil branch target is not instruction-aligned",
        });
    }
    let imm = delta / 4;
    if !(-(1 << 18)..=(1 << 18) - 1).contains(&imm) {
        return Err(StencilError::Unsupported {
            path: "$code".to_owned(),
            reason: "stencil branch target exceeds AArch64 imm19 range",
        });
    }
    Ok(base | (((imm as u32) & 0x7ffff) << 5))
}

#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
fn patch_compare_zero_branch_imm19(
    base: u32,
    branch_offset: usize,
    target: usize,
) -> Result<u32, StencilError> {
    patch_cond_branch_imm19(base, branch_offset, target)
}

#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
fn patch_uncond_branch_imm26(
    base: u32,
    branch_offset: usize,
    target: usize,
) -> Result<u32, StencilError> {
    let branch_offset = isize::try_from(branch_offset).map_err(|_| StencilError::Unsupported {
        path: "$code".to_owned(),
        reason: "stencil branch offset exceeds isize",
    })?;
    let target = isize::try_from(target).map_err(|_| StencilError::Unsupported {
        path: "$code".to_owned(),
        reason: "stencil branch target exceeds isize",
    })?;
    let delta = target
        .checked_sub(branch_offset)
        .ok_or_else(|| StencilError::Unsupported {
            path: "$code".to_owned(),
            reason: "stencil branch offset overflow",
        })?;
    if delta % 4 != 0 {
        return Err(StencilError::Unsupported {
            path: "$code".to_owned(),
            reason: "stencil branch target is not instruction-aligned",
        });
    }
    let imm = delta / 4;
    if !(-(1 << 25)..=(1 << 25) - 1).contains(&imm) {
        return Err(StencilError::Unsupported {
            path: "$code".to_owned(),
            reason: "stencil branch target exceeds AArch64 imm26 range",
        });
    }
    Ok(base | ((imm as u32) & 0x03ff_ffff))
}

#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
fn patch_test_bit_branch_imm14(
    base: u32,
    branch_offset: usize,
    target: usize,
) -> Result<u32, StencilError> {
    let branch_offset = isize::try_from(branch_offset).map_err(|_| StencilError::Unsupported {
        path: "$code".to_owned(),
        reason: "stencil branch offset exceeds isize",
    })?;
    let target = isize::try_from(target).map_err(|_| StencilError::Unsupported {
        path: "$code".to_owned(),
        reason: "stencil branch target exceeds isize",
    })?;
    let delta = target
        .checked_sub(branch_offset)
        .ok_or_else(|| StencilError::Unsupported {
            path: "$code".to_owned(),
            reason: "stencil branch offset overflow",
        })?;
    if delta % 4 != 0 {
        return Err(StencilError::Unsupported {
            path: "$code".to_owned(),
            reason: "stencil branch target is not instruction-aligned",
        });
    }
    let imm = delta / 4;
    if !(-(1 << 13)..=(1 << 13) - 1).contains(&imm) {
        return Err(StencilError::Unsupported {
            path: "$code".to_owned(),
            reason: "stencil branch target exceeds AArch64 imm14 range",
        });
    }
    Ok(base | (((imm as u32) & 0x3fff) << 5))
}

pub(super) fn status_for_failure(index: usize) -> Result<u32, StencilError> {
    u32::try_from(index)
        .ok()
        .and_then(|index| index.checked_add(1))
        .ok_or_else(|| StencilError::Unsupported {
            path: "$code".to_owned(),
            reason: "too many stencil failure stubs",
        })
}

#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
fn emit_mov_x_immediate(code: &mut Vec<u8>, rd: u8, value: u64) -> Result<(), StencilError> {
    if rd > 31 {
        return Err(StencilError::Unsupported {
            path: "$code".to_owned(),
            reason: "stencil register index exceeds AArch64 range",
        });
    }
    let rd = u32::from(rd);
    for shift_index in 0..4 {
        let imm = ((value >> (shift_index * 16)) & 0xffff) as u32;
        let word = if shift_index == 0 {
            0xD280_0000 | (imm << 5) | rd
        } else {
            0xF280_0000 | ((shift_index as u32) << 21) | (imm << 5) | rd
        };
        push_u32(code, word);
    }
    Ok(())
}

#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
fn mov_x_register(rd: u8, rm: u8) -> Result<u32, StencilError> {
    if rd > 31 || rm > 31 {
        return Err(StencilError::Unsupported {
            path: "$code".to_owned(),
            reason: "stencil register index exceeds AArch64 range",
        });
    }
    Ok(0xAA00_03E0 | (u32::from(rm) << 16) | u32::from(rd))
}

#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
fn add_x_immediate(rd: u8, rn: u8, value: usize, path: &str) -> Result<u32, StencilError> {
    if rd > 31 || rn > 31 {
        return Err(StencilError::Unsupported {
            path: "$code".to_owned(),
            reason: "stencil register index exceeds AArch64 range",
        });
    }
    if value > 0xfff {
        return Err(StencilError::Unsupported {
            path: path.to_owned(),
            reason: "stencil add immediate exceeds AArch64 imm12 range",
        });
    }
    Ok(0x9100_0000 | ((value as u32) << 10) | (u32::from(rn) << 5) | u32::from(rd))
}

#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
fn sub_x_immediate(rd: u8, rn: u8, value: usize, path: &str) -> Result<u32, StencilError> {
    if rd > 31 || rn > 31 {
        return Err(StencilError::Unsupported {
            path: "$code".to_owned(),
            reason: "stencil register index exceeds AArch64 range",
        });
    }
    if value > 0xfff {
        return Err(StencilError::Unsupported {
            path: path.to_owned(),
            reason: "stencil sub immediate exceeds AArch64 imm12 range",
        });
    }
    Ok(0xD100_0000 | ((value as u32) << 10) | (u32::from(rn) << 5) | u32::from(rd))
}

#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
fn add_x_register(rd: u8, rn: u8, rm: u8) -> Result<u32, StencilError> {
    if rd > 31 || rn > 31 || rm > 31 {
        return Err(StencilError::Unsupported {
            path: "$code".to_owned(),
            reason: "stencil register index exceeds AArch64 range",
        });
    }
    Ok(0x8B00_0000 | (u32::from(rm) << 16) | (u32::from(rn) << 5) | u32::from(rd))
}

#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
fn stur_w_register(rt: u8, rn: u8, offset: usize, path: &str) -> Result<u32, StencilError> {
    if rt > 31 || rn > 31 {
        return Err(StencilError::Unsupported {
            path: "$code".to_owned(),
            reason: "stencil register index exceeds AArch64 range",
        });
    }
    patch_ldur_stur_imm9(
        0xB800_0000 | (u32::from(rn) << 5) | u32::from(rt),
        offset,
        path,
    )
}

#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
fn ldur_w_register(rt: u8, rn: u8, offset: usize, path: &str) -> Result<u32, StencilError> {
    if rt > 31 || rn > 31 {
        return Err(StencilError::Unsupported {
            path: "$code".to_owned(),
            reason: "stencil register index exceeds AArch64 range",
        });
    }
    patch_ldur_stur_imm9(
        0xB840_0000 | (u32::from(rn) << 5) | u32::from(rt),
        offset,
        path,
    )
}

#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
fn ldur_x_register(rt: u8, rn: u8, offset: usize, path: &str) -> Result<u32, StencilError> {
    if rt > 31 || rn > 31 {
        return Err(StencilError::Unsupported {
            path: "$code".to_owned(),
            reason: "stencil register index exceeds AArch64 range",
        });
    }
    patch_ldur_stur_imm9(
        0xF840_0000 | (u32::from(rn) << 5) | u32::from(rt),
        offset,
        path,
    )
}

#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
fn stur_b_register(rt: u8, rn: u8, offset: usize, path: &str) -> Result<u32, StencilError> {
    if rt > 31 || rn > 31 {
        return Err(StencilError::Unsupported {
            path: "$code".to_owned(),
            reason: "stencil register index exceeds AArch64 range",
        });
    }
    patch_ldur_stur_imm9(
        0x3800_0000 | (u32::from(rn) << 5) | u32::from(rt),
        offset,
        path,
    )
}

#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
fn patch_registers(word: &mut [u8], rt: u8, rn: u8) -> Result<(), StencilError> {
    if word.len() != 4 || rt > 31 || rn > 31 {
        return Err(StencilError::Unsupported {
            path: "$code".to_owned(),
            reason: "stencil register patch is invalid",
        });
    }
    let mut raw = u32::from_le_bytes(word.try_into().unwrap());
    raw &= !0x3ff;
    raw |= u32::from(rt) | (u32::from(rn) << 5);
    word.copy_from_slice(&raw.to_le_bytes());
    Ok(())
}

#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
fn cmp_x_immediate(rn: u8, value: usize, path: &str) -> Result<u32, StencilError> {
    if rn > 31 {
        return Err(StencilError::Unsupported {
            path: "$code".to_owned(),
            reason: "stencil register index exceeds AArch64 range",
        });
    }
    if value > 0xfff {
        return Err(StencilError::Unsupported {
            path: path.to_owned(),
            reason: "stencil compare immediate exceeds AArch64 imm12 range",
        });
    }
    Ok(0xF100_001F | ((value as u32) << 10) | (u32::from(rn) << 5))
}

#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
fn cmp_x_register(rn: u8, rm: u8) -> Result<u32, StencilError> {
    if rn > 31 || rm > 31 {
        return Err(StencilError::Unsupported {
            path: "$code".to_owned(),
            reason: "stencil register index exceeds AArch64 range",
        });
    }
    Ok(0xEB00_001F | (u32::from(rm) << 16) | (u32::from(rn) << 5))
}

#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
fn mov_w0_immediate(value: u32) -> Result<u32, StencilError> {
    if value > u16::MAX as u32 {
        return Err(StencilError::Unsupported {
            path: "$code".to_owned(),
            reason: "stencil status exceeds mov immediate range",
        });
    }
    Ok(0x5280_0000 | (value << 5))
}

#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
fn mov_w10_immediate(value: u32) -> Result<u32, StencilError> {
    if value > u16::MAX as u32 {
        return Err(StencilError::Unsupported {
            path: "$code".to_owned(),
            reason: "stencil immediate exceeds mov immediate range",
        });
    }
    Ok(0x5280_000A | (value << 5))
}

#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
fn cmp_w9_immediate(value: u32) -> Result<u32, StencilError> {
    if value > 0xfff {
        return Err(StencilError::Unsupported {
            path: "$code".to_owned(),
            reason: "stencil enum variant index exceeds cmp immediate range",
        });
    }
    Ok(0x7100_013F | (value << 10))
}

#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
fn push_u32(code: &mut Vec<u8>, word: u32) {
    code.extend_from_slice(&word.to_le_bytes());
}
