// JIT compiler: BytecodeInstruction[256] → native ARM64 code
//
// Register allocation:
//   r[0..7]      → x8..x15
//   scratchpad   → x16
//   e_mask[0]    → x19
//   e_mask[1]    → x20
//   nreg pointer → x21
//   FSCAL mask   → d24 (0x80F0000000000000)
//   temp regs    → x0..x7
//   f[0..3]      → d0/d1, d2/d3, d4/d5, d6/d7  (lo/hi pairs)
//   e[0..3]      → d8/d9, d10/d11, d12/d13, d14/d15
//   a[0..3]      → d16/d17, d18/d19, d20/d21, d22/d23

use super::aarch64::{reg, Emitter};
use super::memory::{JitMemory, JIT_CODE_SIZE};
use crate::randomx::vm::{
    BytecodeInstruction, InstructionType, NativeRegisterFile, ProgramConfiguration,
    RxVersion, RANDOMX_PROGRAM_SIZE_MAX,
};

/// DYNAMIC_MANTISSA_MASK from vm.rs
const DYNAMIC_MANTISSA_MASK: u64 = (1u64 << 56) - 1; // (1 << (52 + 4)) - 1

/// FSCAL XOR mask
const FSCAL_MASK: u64 = 0x80F0000000000000u64;

/// Map RandomX register index (0..7) to ARM64 register (x8..x15)
#[inline]
fn r_reg(idx: usize) -> u32 {
    (idx as u32) + 8
}

/// Map RandomX f-register index (0..3) to ARM64 FP register pair (d0/d1..d6/d7)
/// Returns (lo_dreg, hi_dreg)
#[inline]
fn f_regs(idx: usize) -> (u32, u32) {
    let base = (idx as u32) * 2;
    (base, base + 1)
}

/// Map RandomX e-register index (0..3) to ARM64 FP register pair (d8/d9..d14/d15)
#[inline]
fn e_regs(idx: usize) -> (u32, u32) {
    let base = 8 + (idx as u32) * 2;
    (base, base + 1)
}

/// Map RandomX a-register index (0..3) to ARM64 FP register pair (d16/d17..d22/d23)
#[inline]
fn a_regs(idx: usize) -> (u32, u32) {
    let base = 16 + (idx as u32) * 2;
    (base, base + 1)
}

/// JIT function signature: (nreg, scratchpad, config) -> ()
/// Body-only compilation: the caller drives the 2048-iteration loop.
pub(crate) type JitFn = unsafe extern "C" fn(
    nreg: *mut NativeRegisterFile,
    scratchpad: *mut u8,
    config: *const ProgramConfiguration,
);

/// Native-loop JIT signature: (nreg, scratchpad, dataset, iterations) -> ()
/// The emitted code runs the whole iteration loop itself, keeping the RandomX
/// register file resident across iterations. `config` is not passed — its
/// contents are baked in at compile time (readReg0..3) or held in registers
/// (e_mask, in x19/x20).
///
/// See DESIGN_JIT_NATIVE_LOOP.md. Note `dataset` arrives in x2 and `iterations`
/// in x3, both of which `emit_cfround` uses as scratch, so the prologue must
/// capture them before emitting any body code.
#[allow(dead_code)] // wired up in stage C
pub(crate) type JitLoopFn = unsafe extern "C" fn(
    nreg: *mut NativeRegisterFile,
    scratchpad: *mut u8,
    dataset: *const u8,
    iterations: u64,
    // loop_state_out: receives [ma, mx, sp_addr0, sp_addr1] zero-extended to
    // u64 when the loop exits. Written once, so it costs nothing per iteration.
    // Required by the differential test: ma/mx are not consumed until the
    // *following* iteration, so comparing only the register file and scratchpad
    // cannot localise an ordering error (design D2).
    loop_state_out: *mut u64,
);

/// Which ABI the currently-resident code was compiled for.
///
/// `JitMemory::as_fn` is a `transmute_copy` guarded only by a pointer-size
/// assert, and both signatures are pointer-sized — so nothing would catch
/// calling native-loop code through [`JitFn`]. That mis-call is silent and
/// nasty: x2 (a dataset pointer) would be dereferenced as a
/// `*const ProgramConfiguration`. Both kinds are alive at once during the
/// staged rollout, so the kind is tracked and asserted on every fetch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CompiledKind {
    /// Program body only; caller drives the loop.
    Body,
    /// Whole iteration loop emitted natively.
    NativeLoop,
}

pub struct JitCompiler {
    memory: JitMemory,
    /// Reused across compiles — a fresh `Emitter` per compile meant a 16 KB
    /// allocation 8 times per hash (~55% of compile cost, measured).
    emitter: Emitter,
    /// ABI of the resident code; guards the `get_*_fn` transmutes.
    kind: Option<CompiledKind>,
}

impl JitCompiler {
    pub fn new() -> Result<Self, &'static str> {
        Ok(JitCompiler {
            memory: JitMemory::new(JIT_CODE_SIZE)?,
            emitter: Emitter::new(),
            kind: None,
        })
    }

    /// Compile a bytecode program to native ARM64 code.
    /// `bytecode` holds exactly the program's instructions (256 for rx/0,
    /// 384 for rx/2) — callers slice their max-size buffer to the program size.
    /// `version` selects version-dependent emission (v2: conditional CFROUND).
    pub(crate) fn compile(&mut self, bytecode: &[BytecodeInstruction], version: RxVersion) {
        {
            let e = &mut self.emitter;
            e.clear();
            emit_prologue(e);
            emit_body(e, bytecode, version);
            emit_epilogue(e);
        }
        self.memory.write_code(&self.emitter.code);
        self.kind = Some(CompiledKind::Body);
    }

    /// Get the JIT function pointer.
    ///
    /// # Safety
    /// `compile` must have been called first; the returned function must be
    /// invoked with valid nreg/scratchpad/config pointers.
    pub(crate) unsafe fn get_fn(&self) -> JitFn { unsafe {
        debug_assert_eq!(
            self.kind,
            Some(CompiledKind::Body),
            "get_fn() on code compiled for a different ABI — see CompiledKind"
        );
        self.memory.as_fn::<JitFn>()
    }}

    /// Get the native-loop function pointer.
    ///
    /// # Safety
    /// Native-loop code must have been compiled first; the returned function
    /// must be invoked with valid nreg/scratchpad/dataset pointers and the
    /// dataset must be at least `DATASET_TOTAL_SIZE` bytes.
    #[allow(dead_code)] // wired up in stage C
    pub(crate) unsafe fn get_loop_fn(&self) -> JitLoopFn { unsafe {
        debug_assert_eq!(
            self.kind,
            Some(CompiledKind::NativeLoop),
            "get_loop_fn() on code compiled for a different ABI — see CompiledKind"
        );
        self.memory.as_fn::<JitLoopFn>()
    }}
}

// ============================================================================
// Prologue: save callee-saved regs, load register file into ARM registers
// ============================================================================

fn emit_prologue(e: &mut Emitter) {
    // Save frame pointer and link register
    e.stp_pre(reg::FP, reg::LR, reg::SP, -16);

    // Save callee-saved integer registers x19-x28
    e.stp_pre(reg::X19, reg::X20, reg::SP, -16);
    e.stp_pre(reg::X21, reg::X22, reg::SP, -16);
    e.stp_pre(reg::X23, reg::X24, reg::SP, -16);
    e.stp_pre(reg::X25, reg::X26, reg::SP, -16);
    e.stp_pre(reg::X27, reg::X28, reg::SP, -16);

    // Save callee-saved FP registers d8-d15
    e.stp_fp_pre(reg::D8, reg::D9, reg::SP, -16);
    e.stp_fp_pre(reg::D10, reg::D11, reg::SP, -16);
    e.stp_fp_pre(reg::D12, reg::D13, reg::SP, -16);
    e.stp_fp_pre(reg::D14, reg::D15, reg::SP, -16);

    // Arguments: x0 = nreg, x1 = scratchpad, x2 = config
    // Save nreg pointer for epilogue store-back
    e.mov_reg(reg::X21, reg::X0);

    // Set scratchpad base
    e.mov_reg(reg::X16, reg::X1);

    // Load e_mask[0] and e_mask[1] from config
    // ProgramConfiguration layout (repr(C)):
    //   offset 0:  e_mask[0]: u64
    //   offset 8:  e_mask[1]: u64
    //   offset 16: read_reg0: usize (8 bytes on 64-bit)
    //   ...
    e.ldr_imm(reg::X19, reg::X2, 0);  // e_mask[0]
    e.ldr_imm(reg::X20, reg::X2, 8);  // e_mask[1]

    // Load integer registers r[0..7] from nreg into x8..x15
    // NativeRegisterFile layout (repr(C)):
    //   offset 0: r[0..7] = 8 × u64 = 64 bytes
    for i in 0..8u32 {
        e.ldr_imm(8 + i, reg::X0, i * 8);
    }

    // Load f-registers from nreg into d0..d7
    // offset 64: f[0..3] = 4 × (f64, f64) = 64 bytes
    for i in 0..8u32 {
        e.ldr_fp_imm(i, reg::X0, 64 + i * 8);
    }

    // Load e-registers from nreg into d8..d15
    // offset 128: e[0..3] = 4 × (f64, f64) = 64 bytes
    for i in 0..8u32 {
        e.ldr_fp_imm(8 + i, reg::X0, 128 + i * 8);
    }

    // Load a-registers from nreg into d16..d23
    // offset 192: a[0..3] = 4 × (f64, f64) = 64 bytes
    for i in 0..8u32 {
        e.ldr_fp_imm(16 + i, reg::X0, 192 + i * 8);
    }

    // Load FSCAL mask into d24
    e.mov_imm64(reg::X0, FSCAL_MASK);
    e.fmov_dx(reg::D24, reg::X0);
}

// ============================================================================
// Epilogue: store registers back to nreg, restore callee-saved, return
// ============================================================================

fn emit_epilogue(e: &mut Emitter) {
    // Store r[0..7] back to nreg (x21 = nreg pointer)
    for i in 0..8u32 {
        e.str_imm(8 + i, reg::X21, i * 8);
    }

    // Store f-registers back
    for i in 0..8u32 {
        e.str_fp_imm(i, reg::X21, 64 + i * 8);
    }

    // Store e-registers back
    for i in 0..8u32 {
        e.str_fp_imm(8 + i, reg::X21, 128 + i * 8);
    }

    // Note: a-registers are constant, no need to store back

    // Restore callee-saved FP registers d8-d15
    e.ldp_fp_post(reg::D14, reg::D15, reg::SP, 16);
    e.ldp_fp_post(reg::D12, reg::D13, reg::SP, 16);
    e.ldp_fp_post(reg::D10, reg::D11, reg::SP, 16);
    e.ldp_fp_post(reg::D8, reg::D9, reg::SP, 16);

    // Restore callee-saved integer registers x19-x28
    e.ldp_post(reg::X27, reg::X28, reg::SP, 16);
    e.ldp_post(reg::X25, reg::X26, reg::SP, 16);
    e.ldp_post(reg::X23, reg::X24, reg::SP, 16);
    e.ldp_post(reg::X21, reg::X22, reg::SP, 16);
    e.ldp_post(reg::X19, reg::X20, reg::SP, 16);

    // Restore frame pointer and link register
    e.ldp_post(reg::FP, reg::LR, reg::SP, 16);

    e.ret();
}

// ============================================================================
// Body: compile each bytecode instruction
// ============================================================================

fn emit_body(e: &mut Emitter, bytecode: &[BytecodeInstruction], version: RxVersion) {
    debug_assert!(bytecode.len() <= RANDOMX_PROGRAM_SIZE_MAX);
    // Record instruction offsets for CBRANCH backward jumps
    let mut offsets = [0usize; RANDOMX_PROGRAM_SIZE_MAX];

    for i in 0..bytecode.len() {
        offsets[i] = e.len();
        emit_instruction(e, &bytecode[i], &offsets, i, bytecode.len(), version);
    }
}

fn emit_instruction(
    e: &mut Emitter,
    ibc: &BytecodeInstruction,
    offsets: &[usize; RANDOMX_PROGRAM_SIZE_MAX],
    pc: usize,
    program_size: usize,
    version: RxVersion,
) {
    match ibc.itype {
        InstructionType::IaddRs => emit_iadd_rs(e, ibc),
        InstructionType::IaddM => emit_iadd_m(e, ibc),
        InstructionType::IsubR => emit_isub_r(e, ibc),
        InstructionType::IsubM => emit_isub_m(e, ibc),
        InstructionType::ImulR => emit_imul_r(e, ibc),
        InstructionType::ImulM => emit_imul_m(e, ibc),
        InstructionType::ImulhR => emit_imulh_r(e, ibc),
        InstructionType::ImulhM => emit_imulh_m(e, ibc),
        InstructionType::IsmulhR => emit_ismulh_r(e, ibc),
        InstructionType::IsmulhM => emit_ismulh_m(e, ibc),
        InstructionType::InegR => emit_ineg_r(e, ibc),
        InstructionType::IxorR => emit_ixor_r(e, ibc),
        InstructionType::IxorM => emit_ixor_m(e, ibc),
        InstructionType::IrorR => emit_iror_r(e, ibc),
        InstructionType::IrolR => emit_irol_r(e, ibc),
        InstructionType::IswapR => emit_iswap_r(e, ibc),
        InstructionType::FswapR => emit_fswap_r(e, ibc),
        InstructionType::FaddR => emit_fadd_r(e, ibc),
        InstructionType::FaddM => emit_fadd_m(e, ibc),
        InstructionType::FsubR => emit_fsub_r(e, ibc),
        InstructionType::FsubM => emit_fsub_m(e, ibc),
        InstructionType::FscalR => emit_fscal_r(e, ibc),
        InstructionType::FmulR => emit_fmul_r(e, ibc),
        InstructionType::FdivM => emit_fdiv_m(e, ibc),
        InstructionType::FsqrtR => emit_fsqrt_r(e, ibc),
        InstructionType::Cbranch => emit_cbranch(e, ibc, offsets, pc, program_size),
        InstructionType::Cfround => emit_cfround(e, ibc, version),
        InstructionType::Istore => emit_istore(e, ibc),
        InstructionType::Nop => {}
    }
}

// ============================================================================
// Memory address calculation helper
// Computes: addr = (src_val + imm) as u32 & mem_mask
// Result in x0. Uses x0 as temp.
// ============================================================================

/// Emit scratchpad address calculation into x0.
/// src_val comes from r_reg(src) if src < 8, else 0.
/// Result: x0 = (src_val + imm) as u32 & mem_mask
fn emit_mem_addr(e: &mut Emitter, ibc: &BytecodeInstruction) {
    if ibc.src < 8 {
        // x0 = r[src] + imm (wrapping)
        e.mov_imm64(reg::X0, ibc.imm);
        e.add_reg(reg::X0, r_reg(ibc.src), reg::X0);
    } else {
        // x0 = 0 + imm = imm
        e.mov_imm64(reg::X0, ibc.imm);
    }
    // Truncate to 32-bit and mask. Use AND with bitmask immediate.
    // First truncate: mask with 0xFFFFFFFF, then AND with mem_mask
    // mem_mask is already a 32-bit value, so AND with (mem_mask as u64) works
    // since the upper 32 bits are already 0 after the wrapping add truncation.
    // Actually we need explicit 32-bit truncation: AND x0, x0, #0xFFFFFFFF
    // then AND with mem_mask.
    // Simpler: use W0 register (32-bit view) — the ADD already wraps at 64 bits,
    // but we need only the low 32 bits masked.
    // Let's do: AND x0, x0, #(mem_mask as u64)
    // But mem_mask values like 0x3FF8 need bitmask encoding.
    // Since mem_mask fits in 32 bits and the upper 32 bits of x0 don't matter
    // for the final scratchpad access (offset < 2MB), just AND with mem_mask:
    e.and_bitmask(reg::X0, reg::X0, ibc.mem_mask as u64, reg::X1);
}

/// Load 64-bit value from scratchpad[x0] into tmp register (x0).
/// After this, x0 = scratchpad[addr].
fn emit_mem_load(e: &mut Emitter) {
    // x0 = *(u64*)(x16 + x0)
    e.ldr_reg(reg::X0, reg::X16, reg::X0);
}

// ============================================================================
// Integer instructions
// ============================================================================

fn emit_iadd_rs(e: &mut Emitter, ibc: &BytecodeInstruction) {
    let dst = r_reg(ibc.dst);
    let src = r_reg(ibc.src);
    // dst = dst + (src << shift) + imm
    if ibc.shift > 0 {
        e.add_reg_shifted(dst, dst, src, ibc.shift);
    } else {
        e.add_reg(dst, dst, src);
    }
    if ibc.imm != 0 {
        // Add sign-extended imm32 — may need full 64-bit load
        if ibc.imm < 4096 {
            e.add_imm(dst, dst, ibc.imm as u32);
        } else if (!ibc.imm).wrapping_add(1) < 4096 {
            // Negative imm that fits in sub_imm
            e.sub_imm(dst, dst, (!ibc.imm).wrapping_add(1) as u32);
        } else {
            e.mov_imm64(reg::X0, ibc.imm);
            e.add_reg(dst, dst, reg::X0);
        }
    }
}

fn emit_iadd_m(e: &mut Emitter, ibc: &BytecodeInstruction) {
    emit_mem_addr(e, ibc);
    emit_mem_load(e);
    e.add_reg(r_reg(ibc.dst), r_reg(ibc.dst), reg::X0);
}

fn emit_isub_r(e: &mut Emitter, ibc: &BytecodeInstruction) {
    let dst = r_reg(ibc.dst);
    if ibc.src < 8 {
        e.sub_reg(dst, dst, r_reg(ibc.src));
    } else {
        // Immediate mode
        e.mov_imm64(reg::X0, ibc.imm);
        e.sub_reg(dst, dst, reg::X0);
    }
}

fn emit_isub_m(e: &mut Emitter, ibc: &BytecodeInstruction) {
    emit_mem_addr(e, ibc);
    emit_mem_load(e);
    e.sub_reg(r_reg(ibc.dst), r_reg(ibc.dst), reg::X0);
}

fn emit_imul_r(e: &mut Emitter, ibc: &BytecodeInstruction) {
    let dst = r_reg(ibc.dst);
    if ibc.src < 8 {
        e.mul(dst, dst, r_reg(ibc.src));
    } else {
        e.mov_imm64(reg::X0, ibc.imm);
        e.mul(dst, dst, reg::X0);
    }
}

fn emit_imul_m(e: &mut Emitter, ibc: &BytecodeInstruction) {
    emit_mem_addr(e, ibc);
    emit_mem_load(e);
    e.mul(r_reg(ibc.dst), r_reg(ibc.dst), reg::X0);
}

fn emit_imulh_r(e: &mut Emitter, ibc: &BytecodeInstruction) {
    e.umulh(r_reg(ibc.dst), r_reg(ibc.dst), r_reg(ibc.src));
}

fn emit_imulh_m(e: &mut Emitter, ibc: &BytecodeInstruction) {
    emit_mem_addr(e, ibc);
    emit_mem_load(e);
    e.umulh(r_reg(ibc.dst), r_reg(ibc.dst), reg::X0);
}

fn emit_ismulh_r(e: &mut Emitter, ibc: &BytecodeInstruction) {
    e.smulh(r_reg(ibc.dst), r_reg(ibc.dst), r_reg(ibc.src));
}

fn emit_ismulh_m(e: &mut Emitter, ibc: &BytecodeInstruction) {
    emit_mem_addr(e, ibc);
    emit_mem_load(e);
    e.smulh(r_reg(ibc.dst), r_reg(ibc.dst), reg::X0);
}

fn emit_ineg_r(e: &mut Emitter, ibc: &BytecodeInstruction) {
    e.neg(r_reg(ibc.dst), r_reg(ibc.dst));
}

fn emit_ixor_r(e: &mut Emitter, ibc: &BytecodeInstruction) {
    let dst = r_reg(ibc.dst);
    if ibc.src < 8 {
        e.eor_reg(dst, dst, r_reg(ibc.src));
    } else {
        e.mov_imm64(reg::X0, ibc.imm);
        e.eor_reg(dst, dst, reg::X0);
    }
}

fn emit_ixor_m(e: &mut Emitter, ibc: &BytecodeInstruction) {
    emit_mem_addr(e, ibc);
    emit_mem_load(e);
    e.eor_reg(r_reg(ibc.dst), r_reg(ibc.dst), reg::X0);
}

fn emit_iror_r(e: &mut Emitter, ibc: &BytecodeInstruction) {
    let dst = r_reg(ibc.dst);
    if ibc.src < 8 {
        e.rorv(dst, dst, r_reg(ibc.src));
    } else {
        let imm = (ibc.imm & 63) as u32;
        e.ror_imm(dst, dst, imm);
    }
}

fn emit_irol_r(e: &mut Emitter, ibc: &BytecodeInstruction) {
    let dst = r_reg(ibc.dst);
    if ibc.src < 8 {
        // ROL = ROR by (64 - src)
        e.neg(reg::X0, r_reg(ibc.src));
        e.rorv(dst, dst, reg::X0);
    } else {
        // ROL by immediate = ROR by (64 - imm)
        let imm = (64 - (ibc.imm & 63)) as u32 & 63;
        e.ror_imm(dst, dst, imm);
    }
}

fn emit_iswap_r(e: &mut Emitter, ibc: &BytecodeInstruction) {
    let dst = r_reg(ibc.dst);
    let src = r_reg(ibc.src);
    e.mov_reg(reg::X0, dst);
    e.mov_reg(dst, src);
    e.mov_reg(src, reg::X0);
}

// ============================================================================
// Floating point instructions
// ============================================================================

fn emit_fswap_r(e: &mut Emitter, ibc: &BytecodeInstruction) {
    // Swap lo and hi of a register pair
    let (lo, hi) = if ibc.fswap_is_e {
        e_regs(ibc.dst)
    } else {
        f_regs(ibc.dst)
    };
    // tmp = lo; lo = hi; hi = tmp
    e.fmov_dd(25, lo);  // d25 as temp (not used elsewhere)
    e.fmov_dd(lo, hi);
    e.fmov_dd(hi, 25);
}

fn emit_fadd_r(e: &mut Emitter, ibc: &BytecodeInstruction) {
    let (flo, fhi) = f_regs(ibc.dst);
    let (alo, ahi) = a_regs(ibc.src);
    e.fadd(flo, flo, alo);
    e.fadd(fhi, fhi, ahi);
}

fn emit_fadd_m(e: &mut Emitter, ibc: &BytecodeInstruction) {
    // addr = (r[src] + imm) & mem_mask
    emit_mem_addr(e, ibc);
    // Load two i32 values and convert to f64
    // x0 has the address offset. Load 8 bytes from scratchpad.
    // lo_i32 = *(i32*)(scratchpad + addr)
    // hi_i32 = *(i32*)(scratchpad + addr + 4)
    emit_cvt_packed_int(e);
    // d25 = lo_f64, d26 = hi_f64 (set by emit_cvt_packed_int)
    let (flo, fhi) = f_regs(ibc.dst);
    e.fadd(flo, flo, 25);
    e.fadd(fhi, fhi, 26);
}

fn emit_fsub_r(e: &mut Emitter, ibc: &BytecodeInstruction) {
    let (flo, fhi) = f_regs(ibc.dst);
    let (alo, ahi) = a_regs(ibc.src);
    e.fsub(flo, flo, alo);
    e.fsub(fhi, fhi, ahi);
}

fn emit_fsub_m(e: &mut Emitter, ibc: &BytecodeInstruction) {
    emit_mem_addr(e, ibc);
    emit_cvt_packed_int(e);
    let (flo, fhi) = f_regs(ibc.dst);
    e.fsub(flo, flo, 25);
    e.fsub(fhi, fhi, 26);
}

fn emit_fscal_r(e: &mut Emitter, ibc: &BytecodeInstruction) {
    let (flo, fhi) = f_regs(ibc.dst);
    // XOR with FSCAL mask in d24
    e.eor_v8b(flo, flo, reg::D24);
    e.eor_v8b(fhi, fhi, reg::D24);
}

fn emit_fmul_r(e: &mut Emitter, ibc: &BytecodeInstruction) {
    let (elo, ehi) = e_regs(ibc.dst);
    let (alo, ahi) = a_regs(ibc.src);
    e.fmul(elo, elo, alo);
    e.fmul(ehi, ehi, ahi);
}

fn emit_fdiv_m(e: &mut Emitter, ibc: &BytecodeInstruction) {
    emit_mem_addr(e, ibc);
    emit_cvt_packed_int(e);
    // Apply mask_register_exponent_mantissa to d25, d26:
    // lo_masked = (to_bits(d25) & DYNAMIC_MANTISSA_MASK) | e_mask[0]
    // hi_masked = (to_bits(d26) & DYNAMIC_MANTISSA_MASK) | e_mask[1]
    e.fmov_xd(reg::X0, 25);
    e.and_bitmask(reg::X0, reg::X0, DYNAMIC_MANTISSA_MASK, reg::X1);
    e.orr_reg(reg::X0, reg::X0, reg::X19); // OR with e_mask[0]
    e.fmov_dx(25, reg::X0);

    e.fmov_xd(reg::X0, 26);
    e.and_bitmask(reg::X0, reg::X0, DYNAMIC_MANTISSA_MASK, reg::X1);
    e.orr_reg(reg::X0, reg::X0, reg::X20); // OR with e_mask[1]
    e.fmov_dx(26, reg::X0);

    let (elo, ehi) = e_regs(ibc.dst);
    e.fdiv(elo, elo, 25);
    e.fdiv(ehi, ehi, 26);
}

fn emit_fsqrt_r(e: &mut Emitter, ibc: &BytecodeInstruction) {
    let (elo, ehi) = e_regs(ibc.dst);
    e.fsqrt(elo, elo);
    e.fsqrt(ehi, ehi);
}

// ============================================================================
// Control flow
// ============================================================================

fn emit_cbranch(
    e: &mut Emitter,
    ibc: &BytecodeInstruction,
    offsets: &[usize; RANDOMX_PROGRAM_SIZE_MAX],
    _pc: usize,
    program_size: usize,
) {
    let dst = r_reg(ibc.dst);

    // dst += imm
    e.mov_imm64(reg::X0, ibc.imm);
    e.add_reg(dst, dst, reg::X0);

    // Test: if (dst & mem_mask) == 0, branch backward
    e.tst_bitmask(dst, ibc.mem_mask as u64, reg::X0);

    // Branch target: the interpreter does `pc = target; pc += 1`, so the next
    // instruction executed is target + 1. In the JIT, branch to offsets[target + 1].
    // ibc.target is i16 — cast to i32 first to avoid overflow on negative values.
    let target_signed = (ibc.target as i32) + 1;
    let target_offset = if target_signed >= 0 && (target_signed as usize) < program_size {
        offsets[target_signed as usize]
    } else {
        // Target is out of bounds — fall through (no branch needed)
        e.len() + 1 // make rel == 1 which skips nothing (B.EQ +1 = next instruction)
    };
    let current = e.len();
    let rel = target_offset as i32 - current as i32; // in words
    e.b_cond(0, rel); // B.EQ (cond=0)
}

fn emit_cfround(e: &mut Emitter, ibc: &BytecodeInstruction, version: RxVersion) {
    // rm = (r[src] >> imm) & 3, then map RandomX→ARM rounding mode
    // RandomX: 0→0, 1→2, 2→1, 3→3  (swap bits)
    let src = r_reg(ibc.src);
    let shift = (ibc.imm & 63) as u32;

    // x0 = r[src] rotated right by imm
    if shift > 0 {
        e.ror_imm(reg::X0, src, shift);
    } else {
        e.mov_reg(reg::X0, src);
    }

    // V2: conditional write — skip the FPCR update unless bits 2-5 of the
    // rotated value are all zero (spec §5.4.1; xmrig h_CFROUND emits the same
    // TST #0x3C + B.NE guard). The branch offset is patched after emitting the
    // guarded block so the skip distance never needs hand-counting.
    let patch = if version == RxVersion::V2 {
        e.tst_bitmask(reg::X0, 0x3C, reg::X1);
        let idx = e.len();
        e.b_cond(1, 0); // B.NE placeholder — patched below
        Some(idx)
    } else {
        None
    };

    // x0 = x0 & 3
    e.and_bitmask(reg::X0, reg::X0, 3, reg::X1);

    // Swap bits 0 and 1 to convert RandomX rounding to ARM rounding:
    // ARM: x1 = (x0 >> 1) & 1  (bit 1 becomes bit 0)
    // ARM: x2 = (x0 & 1) << 1  (bit 0 becomes bit 1)
    // ARM: x0 = x1 | x2
    e.lsr_imm(reg::X1, reg::X0, 1);      // x1 = x0 >> 1
    e.lsl_imm(reg::X2, reg::X0, 1);      // x2 = x0 << 1
    e.and_bitmask(reg::X1, reg::X1, 1, reg::X3);  // x1 &= 1
    e.and_bitmask(reg::X2, reg::X2, 2, reg::X3);  // x2 &= 2
    e.orr_reg(reg::X0, reg::X1, reg::X2); // x0 = x1 | x2

    // Set FPCR: FZ=1 (bit 24), RMode in bits [23:22]
    e.lsl_imm(reg::X0, reg::X0, 22);
    e.mov_imm64(reg::X1, 1u64 << 24); // FZ bit
    e.orr_reg(reg::X0, reg::X0, reg::X1);
    e.msr_fpcr(reg::X0);

    if let Some(idx) = patch {
        let rel = (e.len() - idx) as u32; // words from the B.NE to past the block
        e.code[idx] = 0x54000000 | ((rel & 0x7FFFF) << 5) | 1; // B.NE +rel
    }
}

fn emit_istore(e: &mut Emitter, ibc: &BytecodeInstruction) {
    // addr = (r[dst] + imm) as u32 & mem_mask
    // Note: ISTORE uses dst for address calculation, src for value
    if ibc.imm != 0 {
        e.mov_imm64(reg::X0, ibc.imm);
        e.add_reg(reg::X0, r_reg(ibc.dst), reg::X0);
    } else {
        e.mov_reg(reg::X0, r_reg(ibc.dst));
    }
    e.and_bitmask(reg::X0, reg::X0, ibc.mem_mask as u64, reg::X1);
    // Store r[src] to scratchpad[addr]
    e.str_reg(r_reg(ibc.src), reg::X16, reg::X0);
}

// ============================================================================
// Helper: convert packed int to f64 pair
// Input: x0 = scratchpad offset
// Output: d25 = f64(lo_i32), d26 = f64(hi_i32)
// ============================================================================

fn emit_cvt_packed_int(e: &mut Emitter) {
    // x1 = x16 + x0 (absolute address in scratchpad)
    e.add_reg(reg::X1, reg::X16, reg::X0);
    // Load low 32-bit signed int, sign-extend to 64-bit
    // LDRSW x0, [x1, #0]
    e.emit(0xB9800000 | (reg::X1 << 5) | reg::X0); // LDRSW x0, [x1]
    // Convert to f64
    e.scvtf_dx(25, reg::X0);
    // Load high 32-bit signed int
    // LDRSW x0, [x1, #4]
    e.emit(0xB9800400 | (reg::X1 << 5) | reg::X0); // LDRSW x0, [x1, #4]
    e.scvtf_dx(26, reg::X0);
}

// ============================================================================
// Native iteration loop (DESIGN_JIT_NATIVE_LOOP.md)
//
// Emits the whole 2048-iteration loop so the RandomX register file stays
// resident across iterations, instead of being spilled and refilled 16,384
// times per hash by the body-only prologue/epilogue.
//
// v1 + full mode only. Light mode's dataset read is SuperscalarHash and v2
// needs the AES F/E mix; both keep the body-only path.
// ============================================================================

/// Scratchpad address mask: (SCRATCHPAD_L3_SIZE/64 - 1) * 64.
const SCRATCHPAD_L3_MASK64: u64 = 0x1F_FFC0;
/// Dataset address mask. **Memory-safety critical** (design C1): the native
/// loop performs no bounds check, and the worst-case read lands exactly one
/// cache line short of `DATASET_TOTAL_SIZE`. Widening this by a single bit is
/// a 2 GiB-scale out-of-bounds read, not merely a wrong hash.
const CACHE_LINE_ALIGN_MASK: u64 = 0x7FFF_FFC0;

const COND_NE: u32 = 1;

impl JitCompiler {
    /// Compile a program as a self-driving native loop.
    ///
    /// `config`, `init_ma`, `init_mx` and `dataset_offset` are baked in, so the
    /// emitted code takes only (nreg, scratchpad, dataset, iterations).
    #[allow(dead_code, clippy::too_many_arguments)] // wired up in stage C
    pub(crate) fn compile_native_loop(
        &mut self,
        bytecode: &[BytecodeInstruction],
        version: RxVersion,
        config: &ProgramConfiguration,
        init_ma: u32,
        init_mx: u32,
        dataset_offset: u64,
    ) {
        {
            let e = &mut self.emitter;
            e.clear();
            emit_loop_prologue(e, config, init_ma, init_mx, dataset_offset);
            let loop_head = e.len();
            emit_iteration_pre(e, config);
            emit_body(e, bytecode, version);
            emit_iteration_post(e, config);
            // counter -= 1; branch back while non-zero
            e.subs_imm(reg::X28, reg::X28, 1);
            let rel = loop_head as i32 - e.len() as i32;
            e.b_cond(COND_NE, rel);
            emit_loop_epilogue(e);
        }
        self.memory.write_code(&self.emitter.code);
        self.kind = Some(CompiledKind::NativeLoop);
    }
}

/// Save callee-saved state, capture arguments, load the register file once,
/// and initialise loop state.
fn emit_loop_prologue(
    e: &mut Emitter,
    config: &ProgramConfiguration,
    init_ma: u32,
    init_mx: u32,
    dataset_offset: u64,
) {
    e.stp_pre(reg::FP, reg::LR, reg::SP, -16);
    e.stp_pre(reg::X19, reg::X20, reg::SP, -16);
    e.stp_pre(reg::X21, reg::X22, reg::SP, -16);
    e.stp_pre(reg::X23, reg::X24, reg::SP, -16);
    e.stp_pre(reg::X25, reg::X26, reg::SP, -16);
    e.stp_pre(reg::X27, reg::X28, reg::SP, -16);
    e.stp_fp_pre(reg::D8, reg::D9, reg::SP, -16);
    e.stp_fp_pre(reg::D10, reg::D11, reg::SP, -16);
    e.stp_fp_pre(reg::D12, reg::D13, reg::SP, -16);
    e.stp_fp_pre(reg::D14, reg::D15, reg::SP, -16);

    // Capture x2/x3 FIRST — `emit_cfround` uses both as scratch, so the first
    // CFROUND in the body would otherwise destroy the incoming arguments.
    e.mov_reg(reg::X22, reg::X2); // dataset base
    e.mov_reg(reg::X28, reg::X3); // iteration count
    e.mov_reg(reg::X23, reg::X4); // loop-state out-pointer
    e.mov_reg(reg::X21, reg::X0); // nreg
    e.mov_reg(reg::X16, reg::X1); // scratchpad

    // Fold the loop-invariant dataset_offset into the base pointer, so the
    // per-iteration address is just base + (ma & mask).
    e.mov_imm64(reg::X0, dataset_offset);
    e.add_reg(reg::X22, reg::X22, reg::X0);

    // r[0..7] from nreg. f/e are deliberately NOT loaded: both are reassigned
    // from the scratchpad at every loop head, so those 16 loads are dead here.
    for i in 0..8u32 {
        e.ldr_imm(8 + i, reg::X21, i * 8);
    }
    // a[0..3] (loop-invariant, never stored back)
    for i in 0..8u32 {
        e.ldr_fp_imm(16 + i, reg::X21, 192 + i * 8);
    }

    // e_mask stays in registers — `emit_fdiv_m` reads x19/x20 directly — but is
    // now materialised as a constant instead of loaded through a config pointer.
    e.mov_imm64(reg::X19, config.e_mask[0]);
    e.mov_imm64(reg::X20, config.e_mask[1]);

    e.mov_imm64(reg::X0, FSCAL_MASK);
    e.fmov_dx(reg::D24, reg::X0);

    // Loop state: ma, mx, and sp_addr0/sp_addr1 seeded from mx/ma respectively.
    e.mov_imm64(reg::X24, init_ma as u64);
    e.mov_imm64(reg::X25, init_mx as u64);
    e.mov_imm64(reg::X26, init_mx as u64); // sp_addr0 = mx
    e.mov_imm64(reg::X27, init_ma as u64); // sp_addr1 = ma
}

/// Per-iteration work before the program body: scratchpad addressing and the
/// r/f/e loads.
fn emit_iteration_pre(e: &mut Emitter, config: &ProgramConfiguration) {
    // sp_mix = r[readReg0] ^ r[readReg1]   (64-bit)
    e.eor_reg(reg::X0, r_reg(config.read_reg0), r_reg(config.read_reg1));

    // sp_addr0 ^= low32(sp_mix); sp_addr0 &= MASK   (W-form: these are u32)
    e.eor_reg_w(reg::X26, reg::X26, reg::X0);
    e.and_bitmask(reg::X26, reg::X26, SCRATCHPAD_L3_MASK64, reg::X1);
    // sp_addr1 ^= high32(sp_mix); sp_addr1 &= MASK
    e.lsr_imm(reg::X1, reg::X0, 32);
    e.eor_reg_w(reg::X27, reg::X27, reg::X1);
    e.and_bitmask(reg::X27, reg::X27, SCRATCHPAD_L3_MASK64, reg::X1);

    // r[i] ^= scratchpad[sp_addr0 + 8i]  (XOR-accumulate)
    e.add_reg(reg::X2, reg::X16, reg::X26);
    for i in 0..8u32 {
        e.ldr_imm(reg::X3, reg::X2, i * 8);
        e.eor_reg(r_reg(i as usize), r_reg(i as usize), reg::X3);
    }

    // f[i] = cvt_packed_i32(scratchpad[sp_addr1 + 8i])   (ASSIGN, stride 8)
    for i in 0..4usize {
        e.add_imm(reg::X0, reg::X27, (i as u32) * 8);
        emit_cvt_packed_int(e); // -> d25 (lo), d26 (hi)
        let (flo, fhi) = f_regs(i);
        e.fmov_dd(flo, reg::D25);
        e.fmov_dd(fhi, reg::D26);
    }

    // e[i] = mask(cvt_packed_i32(scratchpad[sp_addr1 + 32 + 8i]))
    for i in 0..4usize {
        e.add_imm(reg::X0, reg::X27, 32 + (i as u32) * 8);
        emit_cvt_packed_int(e);
        // (bits & DYNAMIC_MANTISSA_MASK) | e_mask[lane]
        e.fmov_xd(reg::X0, reg::D25);
        e.and_bitmask(reg::X0, reg::X0, DYNAMIC_MANTISSA_MASK, reg::X1);
        e.orr_reg(reg::X0, reg::X0, reg::X19);
        e.fmov_dx(reg::D25, reg::X0);
        e.fmov_xd(reg::X0, reg::D26);
        e.and_bitmask(reg::X0, reg::X0, DYNAMIC_MANTISSA_MASK, reg::X1);
        e.orr_reg(reg::X0, reg::X0, reg::X20);
        e.fmov_dx(reg::D26, reg::X0);
        let (elo, ehi) = e_regs(i);
        e.fmov_dd(elo, reg::D25);
        e.fmov_dd(ehi, reg::D26);
    }
}

/// Per-iteration work after the program body: dataset read, mx/ma update,
/// prefetches, and the register stores.
fn emit_iteration_post(e: &mut Emitter, config: &ProgramConfiguration) {
    // read_ptr = dataset_base_with_offset + (ma & CACHE_LINE_ALIGN_MASK).
    // Captured from the PRE-update ma (design C1 / ordering hazard list).
    e.and_bitmask(reg::X0, reg::X24, CACHE_LINE_ALIGN_MASK, reg::X1);
    e.add_reg(reg::X0, reg::X22, reg::X0);

    // mx ^= (r[readReg2] ^ r[readReg3]) as u32 — BEFORE the dataset XOR.
    // Reversing these two produces wrong hashes (see design §3, defect D1).
    e.eor_reg(reg::X1, r_reg(config.read_reg2), r_reg(config.read_reg3));
    e.eor_reg_w(reg::X25, reg::X25, reg::X1);

    // r[i] ^= dataset[read_ptr + 8i]
    for i in 0..8u32 {
        e.ldr_imm(reg::X2, reg::X0, i * 8);
        e.eor_reg(r_reg(i as usize), r_reg(i as usize), reg::X2);
    }

    // swap(mx, ma)
    e.mov_reg(reg::X1, reg::X24);
    e.mov_reg(reg::X24, reg::X25);
    e.mov_reg(reg::X25, reg::X1);

    // Prefetch the dataset line for a later iteration, from the post-swap ma.
    // The mask is required — an unmasked address silently wastes the prefetch.
    e.and_bitmask(reg::X0, reg::X24, CACHE_LINE_ALIGN_MASK, reg::X1);
    e.prfm_reg(reg::X22, reg::X0);

    // scratchpad[sp_addr1 + 8i] = r[i]
    e.add_reg(reg::X0, reg::X16, reg::X27);
    for i in 0..8u32 {
        e.str_imm(r_reg(i as usize), reg::X0, i * 8);
    }

    // f[i] ^= e[i]  (v1; v2's AES mix keeps the body-only path for now)
    for i in 0..4usize {
        let (flo, fhi) = f_regs(i);
        let (elo, ehi) = e_regs(i);
        e.eor_v8b(flo, flo, elo);
        e.eor_v8b(fhi, fhi, ehi);
    }

    // scratchpad[sp_addr0 + 16i] = f[i]  (16 bytes at stride 16 — note this is
    // NOT the stride the f loads used; see design §3 ordering hazards)
    e.add_reg(reg::X0, reg::X16, reg::X26);
    for i in 0..4usize {
        let (flo, fhi) = f_regs(i);
        e.stp_fp_imm(flo, fhi, reg::X0, (i as i32) * 16);
    }

    // Prefetch next iteration's two scratchpad lines (two, not four — the +64
    // pair was dead: Apple Silicon cache lines are 128 B, see AUDIT 2026-08-29).
    e.eor_reg(reg::X0, r_reg(config.read_reg0), r_reg(config.read_reg1));
    e.and_bitmask(reg::X1, reg::X0, SCRATCHPAD_L3_MASK64, reg::X2);
    e.prfm_reg(reg::X16, reg::X1);
    e.lsr_imm(reg::X1, reg::X0, 32);
    e.and_bitmask(reg::X1, reg::X1, SCRATCHPAD_L3_MASK64, reg::X2);
    e.prfm_reg(reg::X16, reg::X1);

    // sp_addr0 = sp_addr1 = 0 (the next iteration XORs into them)
    e.movz(reg::X26, 0, 0);
    e.movz(reg::X27, 0, 0);
}

/// Store the register file once and restore callee-saved state.
///
/// FPCR is deliberately NOT restored: `emit_cfround` writes the rounding mode
/// and RandomX requires it to carry across program chains. Containment is at
/// the outer boundary in `vm.rs` (`save_rounding_mode` / `restore_rounding_mode`
/// around the whole hash). Making this "ABI-clean" would break consensus.
fn emit_loop_epilogue(e: &mut Emitter) {
    // Loop-carried state out (ma, mx, sp_addr0, sp_addr1), zero-extended.
    e.str_imm(reg::X24, reg::X23, 0);
    e.str_imm(reg::X25, reg::X23, 8);
    e.str_imm(reg::X26, reg::X23, 16);
    e.str_imm(reg::X27, reg::X23, 24);
    for i in 0..8u32 {
        e.str_imm(8 + i, reg::X21, i * 8);
    }
    for i in 0..8u32 {
        e.str_fp_imm(i, reg::X21, 64 + i * 8);
    }
    for i in 0..8u32 {
        e.str_fp_imm(8 + i, reg::X21, 128 + i * 8);
    }
    e.ldp_fp_post(reg::D14, reg::D15, reg::SP, 16);
    e.ldp_fp_post(reg::D12, reg::D13, reg::SP, 16);
    e.ldp_fp_post(reg::D10, reg::D11, reg::SP, 16);
    e.ldp_fp_post(reg::D8, reg::D9, reg::SP, 16);
    e.ldp_post(reg::X27, reg::X28, reg::SP, 16);
    e.ldp_post(reg::X25, reg::X26, reg::SP, 16);
    e.ldp_post(reg::X23, reg::X24, reg::SP, 16);
    e.ldp_post(reg::X21, reg::X22, reg::SP, 16);
    e.ldp_post(reg::X19, reg::X20, reg::SP, 16);
    e.ldp_post(reg::FP, reg::LR, reg::SP, 16);
    e.ret();
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_jit_nop_program() {
        // All NOP program — JIT should load regs, do nothing, store back
        let bytecode: [BytecodeInstruction; RANDOMX_PROGRAM_SIZE_MAX] =
            std::array::from_fn(|_| BytecodeInstruction::new());

        let mut jit = JitCompiler::new().expect("JIT alloc failed");
        jit.compile(&bytecode, RxVersion::V1);

        let mut nreg = NativeRegisterFile::new();
        nreg.r = [1, 2, 3, 4, 5, 6, 7, 8];
        nreg.f[0] = (1.0, 2.0);
        nreg.e[0] = (3.0, 4.0);
        nreg.a[0] = (5.0, 6.0);

        let mut scratchpad = vec![0u8; 2097152]; // 2 MB
        let config = ProgramConfiguration {
            e_mask: [0x3FF0000000000000, 0x3FF0000000000000],
            read_reg0: 0,
            read_reg1: 1,
            read_reg2: 2,
            read_reg3: 3,
        };

        unsafe {
            let f = jit.get_fn();
            f(&mut nreg as *mut _, scratchpad.as_mut_ptr(), &config as *const _);
        }

        // Registers should be unchanged (NOP program)
        assert_eq!(nreg.r, [1, 2, 3, 4, 5, 6, 7, 8]);
        assert_eq!(nreg.f[0], (1.0, 2.0));
        assert_eq!(nreg.e[0], (3.0, 4.0));
        assert_eq!(nreg.a[0], (5.0, 6.0));
    }

    #[test]
    fn test_jit_nop_program_with_fp() {
        // NOP program with non-zero FP values to test FP load/store in prologue/epilogue
        let bytecode: [BytecodeInstruction; RANDOMX_PROGRAM_SIZE_MAX] =
            std::array::from_fn(|_| BytecodeInstruction::new());

        let mut jit = JitCompiler::new().expect("JIT alloc failed");
        jit.compile(&bytecode, RxVersion::V1);

        let mut nreg = NativeRegisterFile::new();
        nreg.r = [0x1111, 0x2222, 0x3333, 0x4444, 0x5555, 0x6666, 0x7777, 0x8888];
        for i in 0..4 {
            nreg.f[i] = (1.0 + i as f64, 2.0 + i as f64);
            nreg.e[i] = (3.0 + i as f64, 4.0 + i as f64);
            nreg.a[i] = (5.0 + i as f64, 6.0 + i as f64);
        }

        let orig_nreg_r = nreg.r;
        let orig_f = nreg.f;
        let orig_e = nreg.e;
        let orig_a = nreg.a;

        let mut scratchpad = vec![0u8; 2097152];
        let config = ProgramConfiguration {
            e_mask: [0x3FF0000000000000, 0x3FF0000000000000],
            read_reg0: 0, read_reg1: 1, read_reg2: 2, read_reg3: 3,
        };

        unsafe {
            let f = jit.get_fn();
            f(&mut nreg as *mut _, scratchpad.as_mut_ptr(), &config as *const _);
        }

        assert_eq!(nreg.r, orig_nreg_r, "r registers mismatch");
        assert_eq!(nreg.f, orig_f, "f registers mismatch");
        assert_eq!(nreg.e, orig_e, "e registers mismatch");
        assert_eq!(nreg.a, orig_a, "a registers mismatch");
    }

    fn make_single_instruction(itype: InstructionType, dst: usize, src: usize, imm: u64, mem_mask: u32, shift: u32) -> [BytecodeInstruction; RANDOMX_PROGRAM_SIZE_MAX] {
        let mut bytecode: [BytecodeInstruction; RANDOMX_PROGRAM_SIZE_MAX] =
            std::array::from_fn(|_| BytecodeInstruction::new());
        bytecode[0].itype = itype;
        bytecode[0].dst = dst;
        bytecode[0].src = src;
        bytecode[0].imm = imm;
        bytecode[0].mem_mask = mem_mask;
        bytecode[0].shift = shift;
        bytecode
    }

    fn run_jit(bytecode: &[BytecodeInstruction; RANDOMX_PROGRAM_SIZE_MAX], nreg: &mut NativeRegisterFile) {
        let mut jit = JitCompiler::new().expect("JIT alloc failed");
        jit.compile(bytecode, RxVersion::V1);
        let mut scratchpad = vec![0u8; 2097152];
        let config = ProgramConfiguration {
            e_mask: [0x3FF0000000000000, 0x3FF0000000000000],
            read_reg0: 0, read_reg1: 1, read_reg2: 2, read_reg3: 3,
        };
        unsafe {
            let f = jit.get_fn();
            f(nreg as *mut _, scratchpad.as_mut_ptr(), &config as *const _);
        }
    }

    #[test] fn test_jit_isub_r() {
        let bytecode = make_single_instruction(InstructionType::IsubR, 0, 1, 0, 0, 0);
        let mut nreg = NativeRegisterFile::new();
        nreg.r[0] = 100; nreg.r[1] = 30;
        run_jit(&bytecode, &mut nreg);
        assert_eq!(nreg.r[0], 70);
    }

    #[test] fn test_jit_isub_r_imm() {
        let bytecode = make_single_instruction(InstructionType::IsubR, 0, 8, 30, 0, 0);
        let mut nreg = NativeRegisterFile::new();
        nreg.r[0] = 100;
        run_jit(&bytecode, &mut nreg);
        assert_eq!(nreg.r[0], 70);
    }

    #[test] fn test_jit_imul_r() {
        let bytecode = make_single_instruction(InstructionType::ImulR, 0, 1, 0, 0, 0);
        let mut nreg = NativeRegisterFile::new();
        nreg.r[0] = 7; nreg.r[1] = 6;
        run_jit(&bytecode, &mut nreg);
        assert_eq!(nreg.r[0], 42);
    }

    #[test] fn test_jit_ixor_r() {
        let bytecode = make_single_instruction(InstructionType::IxorR, 0, 1, 0, 0, 0);
        let mut nreg = NativeRegisterFile::new();
        nreg.r[0] = 0xFF00; nreg.r[1] = 0x0FF0;
        run_jit(&bytecode, &mut nreg);
        assert_eq!(nreg.r[0], 0xF0F0);
    }

    #[test] fn test_jit_ineg_r() {
        let bytecode = make_single_instruction(InstructionType::InegR, 0, 0, 0, 0, 0);
        let mut nreg = NativeRegisterFile::new();
        nreg.r[0] = 42;
        run_jit(&bytecode, &mut nreg);
        assert_eq!(nreg.r[0], (!42u64).wrapping_add(1));
    }

    #[test] fn test_jit_iror_r_imm() {
        let bytecode = make_single_instruction(InstructionType::IrorR, 0, 8, 4, 0, 0);
        let mut nreg = NativeRegisterFile::new();
        nreg.r[0] = 0xF0;
        run_jit(&bytecode, &mut nreg);
        assert_eq!(nreg.r[0], 0xF0u64.rotate_right(4));
    }

    #[test] fn test_jit_irol_r_imm() {
        let bytecode = make_single_instruction(InstructionType::IrolR, 0, 8, 4, 0, 0);
        let mut nreg = NativeRegisterFile::new();
        nreg.r[0] = 0xF0;
        run_jit(&bytecode, &mut nreg);
        assert_eq!(nreg.r[0], 0xF0u64.rotate_left(4));
    }

    #[test] fn test_jit_imulh_r() {
        let bytecode = make_single_instruction(InstructionType::ImulhR, 0, 1, 0, 0, 0);
        let mut nreg = NativeRegisterFile::new();
        nreg.r[0] = u64::MAX; nreg.r[1] = 3;
        run_jit(&bytecode, &mut nreg);
        assert_eq!(nreg.r[0], ((u64::MAX as u128 * 3u128) >> 64) as u64);
    }

    #[test] fn test_jit_ismulh_r() {
        let bytecode = make_single_instruction(InstructionType::IsmulhR, 0, 1, 0, 0, 0);
        let mut nreg = NativeRegisterFile::new();
        nreg.r[0] = -5i64 as u64; nreg.r[1] = 3;
        run_jit(&bytecode, &mut nreg);
        let expected = ((-5i64 as i128 * 3i128) >> 64) as u64;
        assert_eq!(nreg.r[0], expected);
    }

    #[test] fn test_jit_iswap_r() {
        let bytecode = make_single_instruction(InstructionType::IswapR, 0, 1, 0, 0, 0);
        let mut nreg = NativeRegisterFile::new();
        nreg.r[0] = 111; nreg.r[1] = 222;
        run_jit(&bytecode, &mut nreg);
        assert_eq!(nreg.r[0], 222);
        assert_eq!(nreg.r[1], 111);
    }

    #[test] fn test_jit_fadd_r() {
        let mut bytecode: [BytecodeInstruction; RANDOMX_PROGRAM_SIZE_MAX] =
            std::array::from_fn(|_| BytecodeInstruction::new());
        bytecode[0].itype = InstructionType::FaddR;
        bytecode[0].dst = 0; // f[0]
        bytecode[0].src = 0; // a[0]
        let mut nreg = NativeRegisterFile::new();
        nreg.f[0] = (1.0, 2.0);
        nreg.a[0] = (10.0, 20.0);
        run_jit(&bytecode, &mut nreg);
        assert_eq!(nreg.f[0], (11.0, 22.0));
    }

    #[test] fn test_jit_fsub_r() {
        let mut bytecode: [BytecodeInstruction; RANDOMX_PROGRAM_SIZE_MAX] =
            std::array::from_fn(|_| BytecodeInstruction::new());
        bytecode[0].itype = InstructionType::FsubR;
        bytecode[0].dst = 0;
        bytecode[0].src = 0;
        let mut nreg = NativeRegisterFile::new();
        nreg.f[0] = (10.0, 20.0);
        nreg.a[0] = (3.0, 5.0);
        run_jit(&bytecode, &mut nreg);
        assert_eq!(nreg.f[0], (7.0, 15.0));
    }

    #[test] fn test_jit_fmul_r() {
        let mut bytecode: [BytecodeInstruction; RANDOMX_PROGRAM_SIZE_MAX] =
            std::array::from_fn(|_| BytecodeInstruction::new());
        bytecode[0].itype = InstructionType::FmulR;
        bytecode[0].dst = 0;
        bytecode[0].src = 0;
        let mut nreg = NativeRegisterFile::new();
        nreg.e[0] = (3.0, 4.0);
        nreg.a[0] = (2.0, 5.0);
        run_jit(&bytecode, &mut nreg);
        assert_eq!(nreg.e[0], (6.0, 20.0));
    }

    #[test] fn test_jit_fsqrt_r() {
        let mut bytecode: [BytecodeInstruction; RANDOMX_PROGRAM_SIZE_MAX] =
            std::array::from_fn(|_| BytecodeInstruction::new());
        bytecode[0].itype = InstructionType::FsqrtR;
        bytecode[0].dst = 0;
        let mut nreg = NativeRegisterFile::new();
        nreg.e[0] = (16.0, 25.0);
        run_jit(&bytecode, &mut nreg);
        assert_eq!(nreg.e[0], (4.0, 5.0));
    }

    #[test] fn test_jit_fscal_r() {
        let mut bytecode: [BytecodeInstruction; RANDOMX_PROGRAM_SIZE_MAX] =
            std::array::from_fn(|_| BytecodeInstruction::new());
        bytecode[0].itype = InstructionType::FscalR;
        bytecode[0].dst = 0;
        let mut nreg = NativeRegisterFile::new();
        nreg.f[0] = (1.0, 2.0);
        let expected = (
            f64::from_bits(f64::to_bits(1.0) ^ FSCAL_MASK),
            f64::from_bits(f64::to_bits(2.0) ^ FSCAL_MASK),
        );
        run_jit(&bytecode, &mut nreg);
        assert_eq!(nreg.f[0], expected);
    }

    #[test] fn test_jit_fswap_r() {
        let mut bytecode: [BytecodeInstruction; RANDOMX_PROGRAM_SIZE_MAX] =
            std::array::from_fn(|_| BytecodeInstruction::new());
        bytecode[0].itype = InstructionType::FswapR;
        bytecode[0].dst = 0;
        bytecode[0].fswap_is_e = false;
        let mut nreg = NativeRegisterFile::new();
        nreg.f[0] = (1.0, 2.0);
        run_jit(&bytecode, &mut nreg);
        assert_eq!(nreg.f[0], (2.0, 1.0));
    }

    #[test] fn test_jit_and_bitmask_l3() {
        // Verify AND bitmask immediate encoding for scratchpad L3 mask (0x1FFFF8).
        // This caught a bug where immr rotation was computed incorrectly.
        use super::super::aarch64::Emitter;
        let (n, immr, imms) = Emitter::encode_bitmask_imm(0x1FFFF8).unwrap();

        // Execute: MOV x0, #0x1FFFFF; AND x0, x0, #0x1FFFF8; RET
        let mut mem = super::super::memory::JitMemory::new(4096).unwrap();
        let mut e = Emitter::new();
        e.mov_imm64(reg::X0, 0x1FFFFF);
        e.and_imm(reg::X0, reg::X0, n, immr, imms);
        e.ret();
        mem.write_code(&e.code);
        let result = unsafe {
            let f: extern "C" fn() -> u64 = mem.as_fn();
            f()
        };
        assert_eq!(result, 0x1FFFF8, "AND bitmask failed: got 0x{:X}", result);
    }

    #[test] fn test_jit_and_bitmask_l1() {
        // Verify L1 scratchpad mask (0x3FF8)
        use super::super::aarch64::Emitter;
        let (n, immr, imms) = Emitter::encode_bitmask_imm(0x3FF8).unwrap();

        let mut mem = super::super::memory::JitMemory::new(4096).unwrap();
        let mut e = Emitter::new();
        e.mov_imm64(reg::X0, 0xFFFFFFFF);
        e.and_imm(reg::X0, reg::X0, n, immr, imms);
        e.ret();
        mem.write_code(&e.code);
        let result = unsafe {
            let f: extern "C" fn() -> u64 = mem.as_fn();
            f()
        };
        assert_eq!(result, 0x3FF8, "L1 AND bitmask failed: got 0x{:X}", result);
    }

    #[test] fn test_jit_and_bitmask_l2() {
        // Verify L2 scratchpad mask (0x3FFF8)
        use super::super::aarch64::Emitter;
        let (n, immr, imms) = Emitter::encode_bitmask_imm(0x3FFF8).unwrap();

        let mut mem = super::super::memory::JitMemory::new(4096).unwrap();
        let mut e = Emitter::new();
        e.mov_imm64(reg::X0, 0xFFFFFFFF);
        e.and_imm(reg::X0, reg::X0, n, immr, imms);
        e.ret();
        mem.write_code(&e.code);
        let result = unsafe {
            let f: extern "C" fn() -> u64 = mem.as_fn();
            f()
        };
        assert_eq!(result, 0x3FFF8, "L2 AND bitmask failed: got 0x{:X}", result);
    }

    #[test] fn test_jit_istore() {
        // ISTORE: store r[src] to scratchpad at addr = (r[dst] + imm) & mem_mask
        let mut bytecode: [BytecodeInstruction; RANDOMX_PROGRAM_SIZE_MAX] =
            std::array::from_fn(|_| BytecodeInstruction::new());
        bytecode[0].itype = InstructionType::Istore;
        bytecode[0].dst = 0;
        bytecode[0].src = 1;
        bytecode[0].imm = 0;
        bytecode[0].mem_mask = 0x1FFFF8; // L3 mask

        let mut jit = JitCompiler::new().expect("JIT alloc failed");
        jit.compile(&bytecode, RxVersion::V1);
        let mut scratchpad = vec![0u8; 2097152];
        let config = ProgramConfiguration {
            e_mask: [0x3FF0000000000000, 0x3FF0000000000000],
            read_reg0: 0, read_reg1: 1, read_reg2: 2, read_reg3: 3,
        };
        let mut nreg = NativeRegisterFile::new();
        nreg.r[0] = 0x100; // address
        nreg.r[1] = 0xDEADBEEF;
        unsafe {
            let f = jit.get_fn();
            f(&mut nreg as *mut _, scratchpad.as_mut_ptr(), &config as *const _);
        }
        let stored = u64::from_le_bytes(scratchpad[0x100..0x108].try_into().unwrap());
        assert_eq!(stored, 0xDEADBEEF);
    }

    #[test] fn test_jit_cfround() {
        let mut bytecode: [BytecodeInstruction; RANDOMX_PROGRAM_SIZE_MAX] =
            std::array::from_fn(|_| BytecodeInstruction::new());
        bytecode[0].itype = InstructionType::Cfround;
        bytecode[0].src = 0;
        bytecode[0].imm = 0;
        let mut nreg = NativeRegisterFile::new();
        nreg.r[0] = 0; // rounding mode 0
        run_jit(&bytecode, &mut nreg);
        // Should not crash — rounding mode is now set
    }

    #[test] fn test_jit_iadd_m() {
        let mut bytecode: [BytecodeInstruction; RANDOMX_PROGRAM_SIZE_MAX] =
            std::array::from_fn(|_| BytecodeInstruction::new());
        bytecode[0].itype = InstructionType::IaddM;
        bytecode[0].dst = 0;
        bytecode[0].src = 1;
        bytecode[0].imm = 0;
        bytecode[0].mem_mask = 0x3FF8; // L1 mask

        let mut jit = JitCompiler::new().expect("JIT alloc failed");
        jit.compile(&bytecode, RxVersion::V1);
        let mut scratchpad = vec![0u8; 2097152];
        // Write value at scratchpad[0x100]
        scratchpad[0x100..0x108].copy_from_slice(&42u64.to_le_bytes());
        let config = ProgramConfiguration {
            e_mask: [0x3FF0000000000000, 0x3FF0000000000000],
            read_reg0: 0, read_reg1: 1, read_reg2: 2, read_reg3: 3,
        };
        let mut nreg = NativeRegisterFile::new();
        nreg.r[0] = 10;
        nreg.r[1] = 0x100; // will be masked to 0x100 & 0x3FF8 = 0x100 (wait, 0x100 = 256, & 0x3FF8 = 0x100)
        unsafe {
            let f = jit.get_fn();
            f(&mut nreg as *mut _, scratchpad.as_mut_ptr(), &config as *const _);
        }
        assert_eq!(nreg.r[0], 10 + 42);
    }

    #[test] fn test_jit_isub_m() {
        let mut bytecode: [BytecodeInstruction; RANDOMX_PROGRAM_SIZE_MAX] =
            std::array::from_fn(|_| BytecodeInstruction::new());
        bytecode[0].itype = InstructionType::IsubM;
        bytecode[0].dst = 0;
        bytecode[0].src = 1;
        bytecode[0].imm = 0;
        bytecode[0].mem_mask = 0x3FF8;

        let mut jit = JitCompiler::new().expect("JIT alloc failed");
        jit.compile(&bytecode, RxVersion::V1);
        let mut scratchpad = vec![0u8; 2097152];
        scratchpad[0x100..0x108].copy_from_slice(&7u64.to_le_bytes());
        let config = ProgramConfiguration {
            e_mask: [0x3FF0000000000000, 0x3FF0000000000000],
            read_reg0: 0, read_reg1: 1, read_reg2: 2, read_reg3: 3,
        };
        let mut nreg = NativeRegisterFile::new();
        nreg.r[0] = 50;
        nreg.r[1] = 0x100;
        unsafe {
            let f = jit.get_fn();
            f(&mut nreg as *mut _, scratchpad.as_mut_ptr(), &config as *const _);
        }
        assert_eq!(nreg.r[0], 50 - 7);
    }

    #[test] fn test_jit_imul_m() {
        let mut bytecode: [BytecodeInstruction; RANDOMX_PROGRAM_SIZE_MAX] =
            std::array::from_fn(|_| BytecodeInstruction::new());
        bytecode[0].itype = InstructionType::ImulM;
        bytecode[0].dst = 0;
        bytecode[0].src = 1;
        bytecode[0].imm = 0;
        bytecode[0].mem_mask = 0x3FF8;

        let mut jit = JitCompiler::new().expect("JIT alloc failed");
        jit.compile(&bytecode, RxVersion::V1);
        let mut scratchpad = vec![0u8; 2097152];
        scratchpad[0x100..0x108].copy_from_slice(&6u64.to_le_bytes());
        let config = ProgramConfiguration {
            e_mask: [0x3FF0000000000000, 0x3FF0000000000000],
            read_reg0: 0, read_reg1: 1, read_reg2: 2, read_reg3: 3,
        };
        let mut nreg = NativeRegisterFile::new();
        nreg.r[0] = 7;
        nreg.r[1] = 0x100;
        unsafe {
            let f = jit.get_fn();
            f(&mut nreg as *mut _, scratchpad.as_mut_ptr(), &config as *const _);
        }
        assert_eq!(nreg.r[0], 42);
    }

    #[test] fn test_jit_ixor_m() {
        let mut bytecode: [BytecodeInstruction; RANDOMX_PROGRAM_SIZE_MAX] =
            std::array::from_fn(|_| BytecodeInstruction::new());
        bytecode[0].itype = InstructionType::IxorM;
        bytecode[0].dst = 0;
        bytecode[0].src = 1;
        bytecode[0].imm = 0;
        bytecode[0].mem_mask = 0x3FF8;

        let mut jit = JitCompiler::new().expect("JIT alloc failed");
        jit.compile(&bytecode, RxVersion::V1);
        let mut scratchpad = vec![0u8; 2097152];
        scratchpad[0x100..0x108].copy_from_slice(&0xFF00u64.to_le_bytes());
        let config = ProgramConfiguration {
            e_mask: [0x3FF0000000000000, 0x3FF0000000000000],
            read_reg0: 0, read_reg1: 1, read_reg2: 2, read_reg3: 3,
        };
        let mut nreg = NativeRegisterFile::new();
        nreg.r[0] = 0xFFFF;
        nreg.r[1] = 0x100;
        unsafe {
            let f = jit.get_fn();
            f(&mut nreg as *mut _, scratchpad.as_mut_ptr(), &config as *const _);
        }
        assert_eq!(nreg.r[0], 0xFFFF ^ 0xFF00);
    }

    #[test] fn test_jit_ldr_str_register_offset() {
        // Regression test: LDR/STR register-offset encoding must use option=011 (LSL).
        // A prior bug used option=000 (UXTB) which caused SIGILL.
        use super::super::aarch64::Emitter;
        let mut buf = vec![0u8; 64]; // buffer for store/load
        let buf_ptr = buf.as_mut_ptr() as u64;

        let mut mem = super::super::memory::JitMemory::new(4096).unwrap();
        let mut e = Emitter::new();
        // x0 = 0xCAFE; x1 = buf_ptr; x2 = 0 (offset)
        e.movz(reg::X0, 0xCAFE, 0);
        e.mov_imm64(reg::X1, buf_ptr);
        e.movz(reg::X2, 0, 0);
        e.str_reg(reg::X0, reg::X1, reg::X2); // STR x0, [x1, x2]
        e.movz(reg::X0, 0, 0); // clear x0
        e.ldr_reg(reg::X0, reg::X1, reg::X2); // LDR x0, [x1, x2]
        e.ret();
        mem.write_code(&e.code);
        let result = unsafe {
            let f: extern "C" fn() -> u64 = mem.as_fn();
            f()
        };
        assert_eq!(result, 0xCAFE, "LDR/STR register offset failed: got 0x{:X}", result);
    }

    #[test]
    fn test_jit_iadd_rs() {
        let mut bytecode: [BytecodeInstruction; RANDOMX_PROGRAM_SIZE_MAX] =
            std::array::from_fn(|_| BytecodeInstruction::new());

        // r[0] += r[1] << 2
        bytecode[0].itype = InstructionType::IaddRs;
        bytecode[0].dst = 0;
        bytecode[0].src = 1;
        bytecode[0].shift = 2;
        bytecode[0].imm = 0;

        let mut jit = JitCompiler::new().expect("JIT alloc failed");
        jit.compile(&bytecode, RxVersion::V1);

        let mut nreg = NativeRegisterFile::new();
        nreg.r[0] = 100;
        nreg.r[1] = 10;

        let mut scratchpad = vec![0u8; 2097152];
        let config = ProgramConfiguration {
            e_mask: [0x3FF0000000000000, 0x3FF0000000000000],
            read_reg0: 0,
            read_reg1: 1,
            read_reg2: 2,
            read_reg3: 3,
        };

        unsafe {
            let f = jit.get_fn();
            f(&mut nreg as *mut _, scratchpad.as_mut_ptr(), &config as *const _);
        }

        // r[0] = 100 + (10 << 2) = 100 + 40 = 140
        assert_eq!(nreg.r[0], 140);
        assert_eq!(nreg.r[1], 10); // unchanged
    }
}
