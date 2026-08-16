// RandomX VM - program execution and full hash calculation
// Reference: RandomX src/vm_interpreted.cpp, src/virtual_machine.cpp, src/bytecode_machine.cpp

use super::aes_hash::{fill_aes_1rx4, fill_aes_4rx4, hash_aes_1rx4, hash_and_fill_aes_1rx4};
use super::argon2d::argon2d_cache;
use super::blake2b::{blake2b, blake2b_256, blake2b_512};
use super::blake2gen::Blake2Generator;
use super::dataset::{init_dataset_item, RandomXDataset};
use super::superscalar::{generate_superscalar, randomx_reciprocal, SuperscalarProgram};
use std::sync::Arc;

// ============================================================================
// Constants
// ============================================================================

pub(crate) const RANDOMX_PROGRAM_SIZE: usize = 256; // V1
pub(crate) const RANDOMX_PROGRAM_SIZE_V2: usize = 384;
/// Max program size across versions — sizes all bytecode buffers (upstream does
/// the same: `Instruction programBuffer[RANDOMX_PROGRAM_MAX_SIZE]`).
pub(crate) const RANDOMX_PROGRAM_SIZE_MAX: usize = 384;
const RANDOMX_PROGRAM_ITERATIONS: usize = 2048;
const RANDOMX_PROGRAM_COUNT: usize = 8;
const REGISTERS_COUNT: usize = 8;
const REGISTER_COUNT_FLT: usize = 4;

const SCRATCHPAD_L3_SIZE: usize = 2_097_152;
const SCRATCHPAD_L2_SIZE: usize = 262_144;
const SCRATCHPAD_L1_SIZE: usize = 16_384;

// Masks as byte offsets (matching C++ convention: (count_of_u64s - 1) * 8)
const SCRATCHPAD_L1_MASK: u32 = (SCRATCHPAD_L1_SIZE / 8 - 1) as u32 * 8;
const SCRATCHPAD_L2_MASK: u32 = (SCRATCHPAD_L2_SIZE / 8 - 1) as u32 * 8;
const SCRATCHPAD_L3_MASK: u32 = (SCRATCHPAD_L3_SIZE / 8 - 1) as u32 * 8;
const SCRATCHPAD_L3_MASK64: u32 = (SCRATCHPAD_L3_SIZE / 64 - 1) as u32 * 64;

const CACHE_LINE_SIZE: usize = 64;
const CACHE_LINE_ALIGN_MASK: u32 = 0x7FFFFFC0; // Verified against C++ reference
const DATASET_EXTRA_ITEMS: u64 = 524287;

const CONDITION_OFFSET: u32 = 8; // RANDOMX_JUMP_OFFSET
const CONDITION_MASK: u32 = (1 << 8) - 1; // (1 << RANDOMX_JUMP_BITS) - 1
const STORE_L3_CONDITION: u32 = 14;

const MANTISSA_SIZE: u32 = 52;
const EXPONENT_SIZE: u32 = 11;
const MANTISSA_MASK: u64 = (1u64 << MANTISSA_SIZE) - 1;
const EXPONENT_MASK: u64 = (1u64 << EXPONENT_SIZE) - 1;
const EXPONENT_BIAS: u64 = 1023;
const DYNAMIC_EXPONENT_BITS: u32 = 4;
const STATIC_EXPONENT_BITS: u32 = 4;
const CONST_EXPONENT_BITS: u64 = 0x300;
const DYNAMIC_MANTISSA_MASK: u64 = (1u64 << (MANTISSA_SIZE + DYNAMIC_EXPONENT_BITS)) - 1;

// Program structure: entropy[16] (128 bytes) + instructions[program_size] (8 bytes each).
// V1: 2176 bytes, V2: 3200 bytes. C++ layout: entropyBuffer[16] first, then programBuffer[].
const ENTROPY_OFFSET: usize = 0; // entropy at the start
const INSTRUCTIONS_OFFSET: usize = 16 * 8; // 128 bytes after entropy

/// RandomX algorithm version. `V1` = rx/0 (current mainnet), `V2` = rx/2
/// (Monero HF v17, tevador/RandomX#317). See RANDOMX_V2_SEMANTICS.md.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RxVersion {
    V1,
    V2,
}

impl RxVersion {
    #[inline]
    pub(crate) fn program_size(self) -> usize {
        match self {
            RxVersion::V1 => RANDOMX_PROGRAM_SIZE,
            RxVersion::V2 => RANDOMX_PROGRAM_SIZE_V2,
        }
    }

    #[inline]
    pub(crate) fn program_bytes_size(self) -> usize {
        INSTRUCTIONS_OFFSET + self.program_size() * 8
    }
}

// Opcode frequency ceiling values (cumulative). Use u16 since CEIL_ISTORE = 256.
const CEIL_IADD_RS: u16 = 16;
const CEIL_IADD_M: u16 = CEIL_IADD_RS + 7;   // 23
const CEIL_ISUB_R: u16 = CEIL_IADD_M + 16;    // 39
const CEIL_ISUB_M: u16 = CEIL_ISUB_R + 7;     // 46
const CEIL_IMUL_R: u16 = CEIL_ISUB_M + 16;    // 62
const CEIL_IMUL_M: u16 = CEIL_IMUL_R + 4;     // 66
const CEIL_IMULH_R: u16 = CEIL_IMUL_M + 4;    // 70
const CEIL_IMULH_M: u16 = CEIL_IMULH_R + 1;   // 71
const CEIL_ISMULH_R: u16 = CEIL_IMULH_M + 4;  // 75
const CEIL_ISMULH_M: u16 = CEIL_ISMULH_R + 1;  // 76
const CEIL_IMUL_RCP: u16 = CEIL_ISMULH_M + 8;  // 84
const CEIL_INEG_R: u16 = CEIL_IMUL_RCP + 2;    // 86
const CEIL_IXOR_R: u16 = CEIL_INEG_R + 15;     // 101
const CEIL_IXOR_M: u16 = CEIL_IXOR_R + 5;      // 106
const CEIL_IROR_R: u16 = CEIL_IXOR_M + 8;      // 114
const CEIL_IROL_R: u16 = CEIL_IROR_R + 2;      // 116
const CEIL_ISWAP_R: u16 = CEIL_IROL_R + 4;     // 120
const CEIL_FSWAP_R: u16 = CEIL_ISWAP_R + 4;    // 124
const CEIL_FADD_R: u16 = CEIL_FSWAP_R + 16;    // 140
const CEIL_FADD_M: u16 = CEIL_FADD_R + 5;      // 145
const CEIL_FSUB_R: u16 = CEIL_FADD_M + 16;     // 161
const CEIL_FSUB_M: u16 = CEIL_FSUB_R + 5;      // 166
const CEIL_FSCAL_R: u16 = CEIL_FSUB_M + 6;     // 172
const CEIL_FMUL_R: u16 = CEIL_FSCAL_R + 32;    // 204
const CEIL_FDIV_M: u16 = CEIL_FMUL_R + 4;      // 208
const CEIL_FSQRT_R: u16 = CEIL_FDIV_M + 6;     // 214
const CEIL_CBRANCH: u16 = CEIL_FSQRT_R + 25;   // 239
const CEIL_CFROUND: u16 = CEIL_CBRANCH + 1;     // 240
#[allow(dead_code)] // last ceiling is the implicit fallback; kept to complete the table
const CEIL_ISTORE: u16 = CEIL_CFROUND + 16;     // 256

// ============================================================================
// Instruction types
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum InstructionType {
    IaddRs,
    IaddM,
    IsubR,
    IsubM,
    ImulR,
    ImulM,
    ImulhR,
    ImulhM,
    IsmulhR,
    IsmulhM,
    InegR,
    IxorR,
    IxorM,
    IrorR,
    IrolR,
    IswapR,
    FswapR,
    FaddR,
    FaddM,
    FsubR,
    FsubM,
    FscalR,
    FmulR,
    FdivM,
    FsqrtR,
    Cbranch,
    Cfround,
    Istore,
    Nop,
}

// ============================================================================
// Raw instruction (8 bytes from program buffer)
// ============================================================================

struct RawInstruction {
    opcode: u8,
    dst: u8,
    src: u8,
    mod_: u8,
    imm32: u32,
}

impl RawInstruction {
    fn from_bytes(data: &[u8]) -> Self {
        RawInstruction {
            opcode: data[0],
            dst: data[1],
            src: data[2],
            mod_: data[3],
            imm32: u32::from_le_bytes([data[4], data[5], data[6], data[7]]),
        }
    }

    fn get_mod_mem(&self) -> u32 {
        (self.mod_ % 4) as u32
    }

    fn get_mod_shift(&self) -> u32 {
        ((self.mod_ >> 2) % 4) as u32
    }

    fn get_mod_cond(&self) -> u32 {
        (self.mod_ >> 4) as u32
    }
}

// ============================================================================
// Compiled bytecode instruction
// ============================================================================

pub(crate) struct BytecodeInstruction {
    pub(crate) itype: InstructionType,
    pub(crate) dst: usize,  // register index
    pub(crate) src: usize,  // register index (or 8 for "zero"/immediate)
    pub(crate) imm: u64,
    pub(crate) mem_mask: u32,
    pub(crate) shift: u32,
    pub(crate) target: i16,  // for CBRANCH
    // For FP instructions: dst_is_e indicates whether dst references e[] or f[]
    pub(crate) dst_is_e: bool,
    // For FSWAP: whether dst is in e-register group
    pub(crate) fswap_is_e: bool,
}

impl BytecodeInstruction {
    pub(crate) fn new() -> Self {
        BytecodeInstruction {
            itype: InstructionType::Nop,
            dst: 0,
            src: 0,
            imm: 0,
            mem_mask: 0,
            shift: 0,
            target: 0,
            dst_is_e: false,
            fswap_is_e: false,
        }
    }
}

// ============================================================================
// VM register file
// ============================================================================

#[repr(C)]
pub(crate) struct NativeRegisterFile {
    pub(crate) r: [u64; REGISTERS_COUNT],
    pub(crate) f: [(f64, f64); REGISTER_COUNT_FLT],
    pub(crate) e: [(f64, f64); REGISTER_COUNT_FLT],
    pub(crate) a: [(f64, f64); REGISTER_COUNT_FLT],
}

impl NativeRegisterFile {
    pub(crate) fn new() -> Self {
        NativeRegisterFile {
            r: [0u64; REGISTERS_COUNT],
            f: [(0.0, 0.0); REGISTER_COUNT_FLT],
            e: [(0.0, 0.0); REGISTER_COUNT_FLT],
            a: [(0.0, 0.0); REGISTER_COUNT_FLT],
        }
    }

    /// Unchecked register access — indices are always < 8 (masked during compilation).
    #[inline(always)]
    unsafe fn r(&self, i: usize) -> u64 { unsafe {
        *self.r.get_unchecked(i)
    }}
    #[inline(always)]
    unsafe fn r_mut(&mut self, i: usize) -> &mut u64 { unsafe {
        self.r.get_unchecked_mut(i)
    }}
    #[inline(always)]
    unsafe fn f(&self, i: usize) -> (f64, f64) { unsafe {
        *self.f.get_unchecked(i)
    }}
    #[inline(always)]
    unsafe fn f_mut(&mut self, i: usize) -> &mut (f64, f64) { unsafe {
        self.f.get_unchecked_mut(i)
    }}
    #[inline(always)]
    unsafe fn e(&self, i: usize) -> (f64, f64) { unsafe {
        *self.e.get_unchecked(i)
    }}
    #[inline(always)]
    unsafe fn e_mut(&mut self, i: usize) -> &mut (f64, f64) { unsafe {
        self.e.get_unchecked_mut(i)
    }}
    #[inline(always)]
    unsafe fn a(&self, i: usize) -> (f64, f64) { unsafe {
        *self.a.get_unchecked(i)
    }}
}

#[repr(C)]
pub(crate) struct ProgramConfiguration {
    pub(crate) e_mask: [u64; 2],
    pub(crate) read_reg0: usize,
    pub(crate) read_reg1: usize,
    pub(crate) read_reg2: usize,
    pub(crate) read_reg3: usize,
}

// ============================================================================
// Helper functions
// ============================================================================

#[inline(always)]
fn sign_extend_2s_compl(x: u32) -> u64 {
    x as i32 as i64 as u64
}

#[inline(always)]
fn load64(scratchpad: &[u8], offset: usize) -> u64 {
    // Safety: callers always mask offset to be within scratchpad bounds
    // (SCRATCHPAD_L1/L2/L3_MASK ensures offset + 8 <= SCRATCHPAD_L3_SIZE)
    unsafe {
        (scratchpad.as_ptr().add(offset) as *const u64).read_unaligned()
    }
}

#[inline(always)]
fn store64(scratchpad: &mut [u8], offset: usize, val: u64) {
    unsafe {
        (scratchpad.as_mut_ptr().add(offset) as *mut u64).write_unaligned(val);
    }
}

#[inline(always)]
fn mulh(a: u64, b: u64) -> u64 {
    ((a as u128 * b as u128) >> 64) as u64
}

#[inline(always)]
fn smulh(a: i64, b: i64) -> u64 {
    ((a as i128 * b as i128) >> 64) as u64
}

#[inline(always)]
fn rotr64(val: u64, count: u64) -> u64 {
    val.rotate_right((count & 63) as u32)
}

#[inline(always)]
fn rotl64(val: u64, count: u64) -> u64 {
    val.rotate_left((count & 63) as u32)
}

/// Check if x is 0 or a power of 2
#[inline(always)]
fn is_zero_or_power_of_2(x: u64) -> bool {
    (x & x.wrapping_sub(1)) == 0
}

// RandomX MXCSR default: FTZ=1, DAZ=1, all exceptions masked, round-to-nearest
#[cfg(target_arch = "x86_64")]
const RX_MXCSR_DEFAULT: u32 = 0x9FC0;

/// Set hardware FP state for RandomX (FTZ, DAZ, exception masks, rounding mode).
#[cfg(target_arch = "x86_64")]
fn set_rounding_mode(mode: u32) {
    unsafe {
        core::arch::x86_64::_mm_setcsr(RX_MXCSR_DEFAULT | ((mode & 3) << 13));
    }
}

#[cfg(target_arch = "x86_64")]
fn save_rounding_mode() -> u32 {
    unsafe { core::arch::x86_64::_mm_getcsr() }
}

#[cfg(target_arch = "x86_64")]
fn restore_rounding_mode(saved: u32) {
    unsafe { core::arch::x86_64::_mm_setcsr(saved); }
}

#[cfg(target_arch = "aarch64")]
fn set_rounding_mode(mode: u32) {
    // ARM FPCR: bit 24 = FZ (flush to zero), bits [23:22] = RMode
    // RandomX RMode mapping: 0=nearest→00, 1=down→10, 2=up→01, 3=truncate→11
    let arm_mode: u64 = match mode & 3 {
        0 => 0,
        1 => 2,
        2 => 1,
        3 => 3,
        _ => unreachable!(),
    };
    unsafe {
        let fpcr: u64;
        core::arch::asm!("mrs {}, fpcr", out(reg) fpcr);
        // Set FZ=1 (bit 24) and RMode (bits 23:22)
        let new_fpcr = (fpcr & !(0x7u64 << 22)) | (1u64 << 24) | (arm_mode << 22);
        core::arch::asm!("msr fpcr, {}", in(reg) new_fpcr);
    }
}

#[cfg(target_arch = "aarch64")]
fn save_rounding_mode() -> u32 {
    let fpcr: u64;
    unsafe { core::arch::asm!("mrs {}, fpcr", out(reg) fpcr); }
    fpcr as u32
}

#[cfg(target_arch = "aarch64")]
fn restore_rounding_mode(saved: u32) {
    unsafe {
        let fpcr = saved as u64;
        core::arch::asm!("msr fpcr, {}", in(reg) fpcr);
    }
}

/// Convert i32 pair from scratchpad to (f64, f64)
/// Reads 8 bytes: first 4 as i32 -> f64 (lo), next 4 as i32 -> f64 (hi)
#[inline(always)]
fn cvt_packed_int_vec_f128(scratchpad: &[u8], offset: usize) -> (f64, f64) {
    unsafe {
        let lo_i32 = (scratchpad.as_ptr().add(offset) as *const i32).read_unaligned();
        let hi_i32 = (scratchpad.as_ptr().add(offset + 4) as *const i32).read_unaligned();
        (lo_i32 as f64, hi_i32 as f64)
    }
}

/// Get small positive float bits from entropy value
fn get_small_positive_float_bits(entropy: u64) -> u64 {
    let exponent = entropy >> 59; // 0..31
    let mantissa = entropy & MANTISSA_MASK;
    let mut exp = exponent.wrapping_add(EXPONENT_BIAS);
    exp &= EXPONENT_MASK;
    exp <<= MANTISSA_SIZE;
    exp | mantissa
}

/// Get static exponent from entropy
fn get_static_exponent(entropy: u64) -> u64 {
    let mut exponent = CONST_EXPONENT_BITS;
    exponent |= (entropy >> (64 - STATIC_EXPONENT_BITS)) << DYNAMIC_EXPONENT_BITS;
    exponent <<= MANTISSA_SIZE;
    exponent
}

/// Get float mask for e-register masking
fn get_float_mask(entropy: u64) -> u64 {
    let mask22bit = (1u64 << 22) - 1;
    (entropy & mask22bit) | get_static_exponent(entropy)
}

/// Apply mantissa/exponent mask to a (f64, f64) pair
#[inline(always)]
fn mask_register_exponent_mantissa(config: &ProgramConfiguration, x: (f64, f64)) -> (f64, f64) {
    let lo_bits = f64::to_bits(x.0);
    let hi_bits = f64::to_bits(x.1);
    let lo_masked = (lo_bits & DYNAMIC_MANTISSA_MASK) | config.e_mask[0];
    let hi_masked = (hi_bits & DYNAMIC_MANTISSA_MASK) | config.e_mask[1];
    (f64::from_bits(lo_masked), f64::from_bits(hi_masked))
}

/// XOR two (f64, f64) pairs at bit level
#[inline(always)]
fn xor_f128(a: (f64, f64), b: (f64, f64)) -> (f64, f64) {
    (
        f64::from_bits(f64::to_bits(a.0) ^ f64::to_bits(b.0)),
        f64::from_bits(f64::to_bits(a.1) ^ f64::to_bits(b.1)),
    )
}

/// (f64, f64) pair -> 16 raw little-endian bytes (lo lane first), bit-exact.
#[inline(always)]
fn f128_to_bytes(v: (f64, f64)) -> [u8; 16] {
    let mut b = [0u8; 16];
    b[..8].copy_from_slice(&v.0.to_bits().to_le_bytes());
    b[8..].copy_from_slice(&v.1.to_bits().to_le_bytes());
    b
}

/// 16 raw bytes -> (f64, f64) pair, bit-exact (values may be NaN/Inf patterns).
#[inline(always)]
fn bytes_to_f128(b: [u8; 16]) -> (f64, f64) {
    (
        f64::from_bits(u64::from_le_bytes(b[..8].try_into().unwrap())),
        f64::from_bits(u64::from_le_bytes(b[8..].try_into().unwrap())),
    )
}

/// RandomX v2 F/E mix on the register file (RANDOMX_V2_SEMANTICS.md §3):
/// 4 single AES rounds per f-register keyed by the live e-registers, which are
/// themselves unchanged. Round-trips through raw bytes so NaN/Inf bit patterns
/// survive exactly (the mixed f's are arbitrary 128-bit values).
fn aes_mix_f_e(nreg: &mut NativeRegisterFile) {
    let mut f = [[0u8; 16]; REGISTER_COUNT_FLT];
    let mut e = [[0u8; 16]; REGISTER_COUNT_FLT];
    for i in 0..REGISTER_COUNT_FLT {
        f[i] = f128_to_bytes(nreg.f[i]);
        e[i] = f128_to_bytes(nreg.e[i]);
    }
    super::aes_hash::aes_mix_fe(&mut f, &e);
    for i in 0..REGISTER_COUNT_FLT {
        nreg.f[i] = bytes_to_f128(f[i]);
    }
}

/// Swap lo and hi of an f128 pair
#[inline(always)]
fn swap_f128(a: (f64, f64)) -> (f64, f64) {
    (a.1, a.0)
}

/// Store (f64, f64) to scratchpad at offset
#[inline(always)]
fn store_f128(scratchpad: &mut [u8], offset: usize, val: (f64, f64)) {
    unsafe {
        (scratchpad.as_mut_ptr().add(offset) as *mut u64).write_unaligned(f64::to_bits(val.0));
        (scratchpad.as_mut_ptr().add(offset + 8) as *mut u64).write_unaligned(f64::to_bits(val.1));
    }
}

/// Serialize the register file to 256 bytes (little-endian) for Blake2b
fn serialize_register_file(nreg: &NativeRegisterFile) -> [u8; 256] {
    let mut out = [0u8; 256];
    // r[0..7]: 8 x u64 = 64 bytes
    for i in 0..REGISTERS_COUNT {
        out[i * 8..(i + 1) * 8].copy_from_slice(&nreg.r[i].to_le_bytes());
    }
    // f[0..3]: 4 x (f64, f64) = 64 bytes
    for i in 0..REGISTER_COUNT_FLT {
        let off = 64 + i * 16;
        out[off..off + 8].copy_from_slice(&f64::to_bits(nreg.f[i].0).to_le_bytes());
        out[off + 8..off + 16].copy_from_slice(&f64::to_bits(nreg.f[i].1).to_le_bytes());
    }
    // e[0..3]: 4 x (f64, f64) = 64 bytes
    for i in 0..REGISTER_COUNT_FLT {
        let off = 128 + i * 16;
        out[off..off + 8].copy_from_slice(&f64::to_bits(nreg.e[i].0).to_le_bytes());
        out[off + 8..off + 16].copy_from_slice(&f64::to_bits(nreg.e[i].1).to_le_bytes());
    }
    // a[0..3]: 4 x (f64, f64) = 64 bytes
    for i in 0..REGISTER_COUNT_FLT {
        let off = 192 + i * 16;
        out[off..off + 8].copy_from_slice(&f64::to_bits(nreg.a[i].0).to_le_bytes());
        out[off + 8..off + 16].copy_from_slice(&f64::to_bits(nreg.a[i].1).to_le_bytes());
    }
    out
}

// ============================================================================
// Program compilation (opcode -> bytecode)
// ============================================================================

fn compile_program(
    program_bytes: &[u8], // raw program bytes (instructions start at offset 128)
    register_usage: &mut [i32; REGISTERS_COUNT],
    bytecode: &mut [BytecodeInstruction; RANDOMX_PROGRAM_SIZE_MAX],
    program_size: usize, // RxVersion::program_size(); entries beyond it are left stale
) {
    debug_assert!(program_size <= RANDOMX_PROGRAM_SIZE_MAX);
    for r in register_usage.iter_mut() {
        *r = -1;
    }

    for i in 0..program_size {
        let instr_offset = INSTRUCTIONS_OFFSET + i * 8; // instructions after entropy
        let instr = RawInstruction::from_bytes(&program_bytes[instr_offset..instr_offset + 8]);
        let ibc = &mut bytecode[i];
        *ibc = BytecodeInstruction::new();

        let opcode = instr.opcode as u16;

        if opcode < CEIL_IADD_RS {
            let dst = (instr.dst % 8) as usize;
            let src = (instr.src % 8) as usize;
            ibc.itype = InstructionType::IaddRs;
            ibc.dst = dst;
            ibc.src = src;
            ibc.shift = instr.get_mod_shift();
            if dst == 5 {
                // RegisterNeedsDisplacement
                ibc.imm = sign_extend_2s_compl(instr.imm32);
            } else {
                ibc.imm = 0;
            }
            register_usage[dst] = i as i32;
        } else if opcode < CEIL_IADD_M {
            let dst = (instr.dst % 8) as usize;
            let src = (instr.src % 8) as usize;
            ibc.itype = InstructionType::IaddM;
            ibc.dst = dst;
            ibc.imm = sign_extend_2s_compl(instr.imm32);
            if src != dst {
                ibc.src = src;
                ibc.mem_mask = if instr.get_mod_mem() != 0 {
                    SCRATCHPAD_L1_MASK
                } else {
                    SCRATCHPAD_L2_MASK
                };
            } else {
                ibc.src = 8; // zero
                ibc.mem_mask = SCRATCHPAD_L3_MASK;
            }
            register_usage[dst] = i as i32;
        } else if opcode < CEIL_ISUB_R {
            let dst = (instr.dst % 8) as usize;
            let src = (instr.src % 8) as usize;
            ibc.itype = InstructionType::IsubR;
            ibc.dst = dst;
            if src != dst {
                ibc.src = src;
            } else {
                ibc.src = 8; // use imm
                ibc.imm = sign_extend_2s_compl(instr.imm32);
            }
            register_usage[dst] = i as i32;
        } else if opcode < CEIL_ISUB_M {
            let dst = (instr.dst % 8) as usize;
            let src = (instr.src % 8) as usize;
            ibc.itype = InstructionType::IsubM;
            ibc.dst = dst;
            ibc.imm = sign_extend_2s_compl(instr.imm32);
            if src != dst {
                ibc.src = src;
                ibc.mem_mask = if instr.get_mod_mem() != 0 {
                    SCRATCHPAD_L1_MASK
                } else {
                    SCRATCHPAD_L2_MASK
                };
            } else {
                ibc.src = 8;
                ibc.mem_mask = SCRATCHPAD_L3_MASK;
            }
            register_usage[dst] = i as i32;
        } else if opcode < CEIL_IMUL_R {
            let dst = (instr.dst % 8) as usize;
            let src = (instr.src % 8) as usize;
            ibc.itype = InstructionType::ImulR;
            ibc.dst = dst;
            if src != dst {
                ibc.src = src;
            } else {
                ibc.src = 8;
                ibc.imm = sign_extend_2s_compl(instr.imm32);
            }
            register_usage[dst] = i as i32;
        } else if opcode < CEIL_IMUL_M {
            let dst = (instr.dst % 8) as usize;
            let src = (instr.src % 8) as usize;
            ibc.itype = InstructionType::ImulM;
            ibc.dst = dst;
            ibc.imm = sign_extend_2s_compl(instr.imm32);
            if src != dst {
                ibc.src = src;
                ibc.mem_mask = if instr.get_mod_mem() != 0 {
                    SCRATCHPAD_L1_MASK
                } else {
                    SCRATCHPAD_L2_MASK
                };
            } else {
                ibc.src = 8;
                ibc.mem_mask = SCRATCHPAD_L3_MASK;
            }
            register_usage[dst] = i as i32;
        } else if opcode < CEIL_IMULH_R {
            let dst = (instr.dst % 8) as usize;
            let src = (instr.src % 8) as usize;
            ibc.itype = InstructionType::ImulhR;
            ibc.dst = dst;
            ibc.src = src;
            register_usage[dst] = i as i32;
        } else if opcode < CEIL_IMULH_M {
            let dst = (instr.dst % 8) as usize;
            let src = (instr.src % 8) as usize;
            ibc.itype = InstructionType::ImulhM;
            ibc.dst = dst;
            ibc.imm = sign_extend_2s_compl(instr.imm32);
            if src != dst {
                ibc.src = src;
                ibc.mem_mask = if instr.get_mod_mem() != 0 {
                    SCRATCHPAD_L1_MASK
                } else {
                    SCRATCHPAD_L2_MASK
                };
            } else {
                ibc.src = 8;
                ibc.mem_mask = SCRATCHPAD_L3_MASK;
            }
            register_usage[dst] = i as i32;
        } else if opcode < CEIL_ISMULH_R {
            let dst = (instr.dst % 8) as usize;
            let src = (instr.src % 8) as usize;
            ibc.itype = InstructionType::IsmulhR;
            ibc.dst = dst;
            ibc.src = src;
            register_usage[dst] = i as i32;
        } else if opcode < CEIL_ISMULH_M {
            let dst = (instr.dst % 8) as usize;
            let src = (instr.src % 8) as usize;
            ibc.itype = InstructionType::IsmulhM;
            ibc.dst = dst;
            ibc.imm = sign_extend_2s_compl(instr.imm32);
            if src != dst {
                ibc.src = src;
                ibc.mem_mask = if instr.get_mod_mem() != 0 {
                    SCRATCHPAD_L1_MASK
                } else {
                    SCRATCHPAD_L2_MASK
                };
            } else {
                ibc.src = 8;
                ibc.mem_mask = SCRATCHPAD_L3_MASK;
            }
            register_usage[dst] = i as i32;
        } else if opcode < CEIL_IMUL_RCP {
            let divisor = instr.imm32;
            if !is_zero_or_power_of_2(divisor as u64) {
                let dst = (instr.dst % 8) as usize;
                ibc.itype = InstructionType::ImulR;
                ibc.dst = dst;
                ibc.src = 8; // use imm
                ibc.imm = randomx_reciprocal(divisor);
                register_usage[dst] = i as i32;
            } else {
                ibc.itype = InstructionType::Nop;
            }
        } else if opcode < CEIL_INEG_R {
            let dst = (instr.dst % 8) as usize;
            ibc.itype = InstructionType::InegR;
            ibc.dst = dst;
            register_usage[dst] = i as i32;
        } else if opcode < CEIL_IXOR_R {
            let dst = (instr.dst % 8) as usize;
            let src = (instr.src % 8) as usize;
            ibc.itype = InstructionType::IxorR;
            ibc.dst = dst;
            if src != dst {
                ibc.src = src;
            } else {
                ibc.src = 8;
                ibc.imm = sign_extend_2s_compl(instr.imm32);
            }
            register_usage[dst] = i as i32;
        } else if opcode < CEIL_IXOR_M {
            let dst = (instr.dst % 8) as usize;
            let src = (instr.src % 8) as usize;
            ibc.itype = InstructionType::IxorM;
            ibc.dst = dst;
            ibc.imm = sign_extend_2s_compl(instr.imm32);
            if src != dst {
                ibc.src = src;
                ibc.mem_mask = if instr.get_mod_mem() != 0 {
                    SCRATCHPAD_L1_MASK
                } else {
                    SCRATCHPAD_L2_MASK
                };
            } else {
                ibc.src = 8;
                ibc.mem_mask = SCRATCHPAD_L3_MASK;
            }
            register_usage[dst] = i as i32;
        } else if opcode < CEIL_IROR_R {
            let dst = (instr.dst % 8) as usize;
            let src = (instr.src % 8) as usize;
            ibc.itype = InstructionType::IrorR;
            ibc.dst = dst;
            if src != dst {
                ibc.src = src;
            } else {
                ibc.src = 8;
                ibc.imm = instr.imm32 as u64;
            }
            register_usage[dst] = i as i32;
        } else if opcode < CEIL_IROL_R {
            let dst = (instr.dst % 8) as usize;
            let src = (instr.src % 8) as usize;
            ibc.itype = InstructionType::IrolR;
            ibc.dst = dst;
            if src != dst {
                ibc.src = src;
            } else {
                ibc.src = 8;
                ibc.imm = instr.imm32 as u64;
            }
            register_usage[dst] = i as i32;
        } else if opcode < CEIL_ISWAP_R {
            let dst = (instr.dst % 8) as usize;
            let src = (instr.src % 8) as usize;
            if src != dst {
                ibc.itype = InstructionType::IswapR;
                ibc.dst = dst;
                ibc.src = src;
                register_usage[dst] = i as i32;
                register_usage[src] = i as i32;
            } else {
                ibc.itype = InstructionType::Nop;
            }
        } else if opcode < CEIL_FSWAP_R {
            let dst = (instr.dst % 8) as usize;
            ibc.itype = InstructionType::FswapR;
            if dst < REGISTER_COUNT_FLT {
                ibc.dst = dst;
                ibc.fswap_is_e = false;
            } else {
                ibc.dst = dst - REGISTER_COUNT_FLT;
                ibc.fswap_is_e = true;
            }
        } else if opcode < CEIL_FADD_R {
            let dst = (instr.dst % REGISTER_COUNT_FLT as u8) as usize;
            let src = (instr.src % REGISTER_COUNT_FLT as u8) as usize;
            ibc.itype = InstructionType::FaddR;
            ibc.dst = dst;
            ibc.src = src;
        } else if opcode < CEIL_FADD_M {
            let dst = (instr.dst % REGISTER_COUNT_FLT as u8) as usize;
            let src = (instr.src % 8) as usize;
            ibc.itype = InstructionType::FaddM;
            ibc.dst = dst;
            ibc.src = src;
            ibc.mem_mask = if instr.get_mod_mem() != 0 {
                SCRATCHPAD_L1_MASK
            } else {
                SCRATCHPAD_L2_MASK
            };
            ibc.imm = sign_extend_2s_compl(instr.imm32);
        } else if opcode < CEIL_FSUB_R {
            let dst = (instr.dst % REGISTER_COUNT_FLT as u8) as usize;
            let src = (instr.src % REGISTER_COUNT_FLT as u8) as usize;
            ibc.itype = InstructionType::FsubR;
            ibc.dst = dst;
            ibc.src = src;
        } else if opcode < CEIL_FSUB_M {
            let dst = (instr.dst % REGISTER_COUNT_FLT as u8) as usize;
            let src = (instr.src % 8) as usize;
            ibc.itype = InstructionType::FsubM;
            ibc.dst = dst;
            ibc.src = src;
            ibc.mem_mask = if instr.get_mod_mem() != 0 {
                SCRATCHPAD_L1_MASK
            } else {
                SCRATCHPAD_L2_MASK
            };
            ibc.imm = sign_extend_2s_compl(instr.imm32);
        } else if opcode < CEIL_FSCAL_R {
            let dst = (instr.dst % REGISTER_COUNT_FLT as u8) as usize;
            ibc.itype = InstructionType::FscalR;
            ibc.dst = dst;
        } else if opcode < CEIL_FMUL_R {
            let dst = (instr.dst % REGISTER_COUNT_FLT as u8) as usize;
            let src = (instr.src % REGISTER_COUNT_FLT as u8) as usize;
            ibc.itype = InstructionType::FmulR;
            ibc.dst = dst;
            ibc.src = src;
            ibc.dst_is_e = true; // FMUL_R targets e[] registers
        } else if opcode < CEIL_FDIV_M {
            let dst = (instr.dst % REGISTER_COUNT_FLT as u8) as usize;
            let src = (instr.src % 8) as usize;
            ibc.itype = InstructionType::FdivM;
            ibc.dst = dst;
            ibc.src = src;
            ibc.dst_is_e = true;
            ibc.mem_mask = if instr.get_mod_mem() != 0 {
                SCRATCHPAD_L1_MASK
            } else {
                SCRATCHPAD_L2_MASK
            };
            ibc.imm = sign_extend_2s_compl(instr.imm32);
        } else if opcode < CEIL_FSQRT_R {
            let dst = (instr.dst % REGISTER_COUNT_FLT as u8) as usize;
            ibc.itype = InstructionType::FsqrtR;
            ibc.dst = dst;
            ibc.dst_is_e = true;
        } else if opcode < CEIL_CBRANCH {
            let creg = (instr.dst % 8) as usize;
            ibc.itype = InstructionType::Cbranch;
            ibc.dst = creg;
            ibc.target = register_usage[creg] as i16;
            let shift = instr.get_mod_cond() + CONDITION_OFFSET;
            ibc.imm = sign_extend_2s_compl(instr.imm32) | (1u64 << shift);
            if CONDITION_OFFSET > 0 || shift > 0 {
                ibc.imm &= !(1u64 << (shift - 1));
            }
            ibc.mem_mask = CONDITION_MASK << shift;
            // Mark all registers as used
            for j in 0..REGISTERS_COUNT {
                register_usage[j] = i as i32;
            }
        } else if opcode < CEIL_CFROUND {
            let src = (instr.src % 8) as usize;
            ibc.itype = InstructionType::Cfround;
            ibc.src = src;
            ibc.imm = (instr.imm32 & 63) as u64;
        } else {
            // ISTORE (opcode < 256, CEIL_ISTORE wraps)
            let dst = (instr.dst % 8) as usize;
            let src = (instr.src % 8) as usize;
            ibc.itype = InstructionType::Istore;
            ibc.dst = dst;
            ibc.src = src;
            ibc.imm = sign_extend_2s_compl(instr.imm32);
            if instr.get_mod_cond() < STORE_L3_CONDITION {
                ibc.mem_mask = if instr.get_mod_mem() != 0 {
                    SCRATCHPAD_L1_MASK
                } else {
                    SCRATCHPAD_L2_MASK
                };
            } else {
                ibc.mem_mask = SCRATCHPAD_L3_MASK;
            }
        }

    }
}

// ============================================================================
// Bytecode execution
// ============================================================================

fn execute_bytecode(
    bytecode: &[BytecodeInstruction], // program_size entries (sliced by caller)
    nreg: &mut NativeRegisterFile,
    scratchpad: &mut [u8],
    config: &ProgramConfiguration,
    version: RxVersion,
) {
    let mut pc: i32 = 0;
    let len = bytecode.len() as i32;

    // Safety: all register indices (dst, src) are < 8 (masked to % 8 during compilation).
    // All scratchpad offsets are masked to be within SCRATCHPAD_L3_SIZE.
    // Bytecode indices are bounded by the program size: pc is only ever set to
    // 0..len via the while condition, and CBRANCH targets are generated < program_size.
    unsafe {
    while pc < len {
        let ibc = bytecode.get_unchecked(pc as usize);
        match ibc.itype {
            InstructionType::IaddRs => {
                let src_val = nreg.r(ibc.src);
                *nreg.r_mut(ibc.dst) = nreg.r(ibc.dst)
                    .wrapping_add((src_val << ibc.shift).wrapping_add(ibc.imm));
            }
            InstructionType::IaddM => {
                let src_val = if ibc.src < 8 { nreg.r(ibc.src) } else { 0 };
                let addr = (src_val.wrapping_add(ibc.imm) as u32 & ibc.mem_mask) as usize;
                *nreg.r_mut(ibc.dst) = nreg.r(ibc.dst).wrapping_add(load64(scratchpad, addr));
            }
            InstructionType::IsubR => {
                let src_val = if ibc.src < 8 { nreg.r(ibc.src) } else { ibc.imm };
                *nreg.r_mut(ibc.dst) = nreg.r(ibc.dst).wrapping_sub(src_val);
            }
            InstructionType::IsubM => {
                let src_val = if ibc.src < 8 { nreg.r(ibc.src) } else { 0 };
                let addr = (src_val.wrapping_add(ibc.imm) as u32 & ibc.mem_mask) as usize;
                *nreg.r_mut(ibc.dst) = nreg.r(ibc.dst).wrapping_sub(load64(scratchpad, addr));
            }
            InstructionType::ImulR => {
                let src_val = if ibc.src < 8 { nreg.r(ibc.src) } else { ibc.imm };
                *nreg.r_mut(ibc.dst) = nreg.r(ibc.dst).wrapping_mul(src_val);
            }
            InstructionType::ImulM => {
                let src_val = if ibc.src < 8 { nreg.r(ibc.src) } else { 0 };
                let addr = (src_val.wrapping_add(ibc.imm) as u32 & ibc.mem_mask) as usize;
                *nreg.r_mut(ibc.dst) = nreg.r(ibc.dst).wrapping_mul(load64(scratchpad, addr));
            }
            InstructionType::ImulhR => {
                *nreg.r_mut(ibc.dst) = mulh(nreg.r(ibc.dst), nreg.r(ibc.src));
            }
            InstructionType::ImulhM => {
                let src_val = if ibc.src < 8 { nreg.r(ibc.src) } else { 0 };
                let addr = (src_val.wrapping_add(ibc.imm) as u32 & ibc.mem_mask) as usize;
                *nreg.r_mut(ibc.dst) = mulh(nreg.r(ibc.dst), load64(scratchpad, addr));
            }
            InstructionType::IsmulhR => {
                *nreg.r_mut(ibc.dst) = smulh(nreg.r(ibc.dst) as i64, nreg.r(ibc.src) as i64);
            }
            InstructionType::IsmulhM => {
                let src_val = if ibc.src < 8 { nreg.r(ibc.src) } else { 0 };
                let addr = (src_val.wrapping_add(ibc.imm) as u32 & ibc.mem_mask) as usize;
                *nreg.r_mut(ibc.dst) = smulh(nreg.r(ibc.dst) as i64, load64(scratchpad, addr) as i64);
            }
            InstructionType::InegR => {
                *nreg.r_mut(ibc.dst) = (!nreg.r(ibc.dst)).wrapping_add(1);
            }
            InstructionType::IxorR => {
                let src_val = if ibc.src < 8 { nreg.r(ibc.src) } else { ibc.imm };
                *nreg.r_mut(ibc.dst) ^= src_val;
            }
            InstructionType::IxorM => {
                let src_val = if ibc.src < 8 { nreg.r(ibc.src) } else { 0 };
                let addr = (src_val.wrapping_add(ibc.imm) as u32 & ibc.mem_mask) as usize;
                *nreg.r_mut(ibc.dst) ^= load64(scratchpad, addr);
            }
            InstructionType::IrorR => {
                let src_val = if ibc.src < 8 { nreg.r(ibc.src) } else { ibc.imm };
                *nreg.r_mut(ibc.dst) = rotr64(nreg.r(ibc.dst), src_val);
            }
            InstructionType::IrolR => {
                let src_val = if ibc.src < 8 { nreg.r(ibc.src) } else { ibc.imm };
                *nreg.r_mut(ibc.dst) = rotl64(nreg.r(ibc.dst), src_val);
            }
            InstructionType::IswapR => {
                let tmp = nreg.r(ibc.dst);
                *nreg.r_mut(ibc.dst) = nreg.r(ibc.src);
                *nreg.r_mut(ibc.src) = tmp;
            }
            InstructionType::FswapR => {
                if ibc.fswap_is_e {
                    *nreg.e_mut(ibc.dst) = swap_f128(nreg.e(ibc.dst));
                } else {
                    *nreg.f_mut(ibc.dst) = swap_f128(nreg.f(ibc.dst));
                }
            }
            InstructionType::FaddR => {
                let (lo, hi) = nreg.f(ibc.dst);
                let (slo, shi) = nreg.a(ibc.src);
                *nreg.f_mut(ibc.dst) = (lo + slo, hi + shi);
            }
            InstructionType::FaddM => {
                let addr = (nreg.r(ibc.src).wrapping_add(ibc.imm) as u32 & ibc.mem_mask) as usize;
                let fsrc = cvt_packed_int_vec_f128(scratchpad, addr);
                let (lo, hi) = nreg.f(ibc.dst);
                *nreg.f_mut(ibc.dst) = (lo + fsrc.0, hi + fsrc.1);
            }
            InstructionType::FsubR => {
                let (lo, hi) = nreg.f(ibc.dst);
                let (slo, shi) = nreg.a(ibc.src);
                *nreg.f_mut(ibc.dst) = (lo - slo, hi - shi);
            }
            InstructionType::FsubM => {
                let addr = (nreg.r(ibc.src).wrapping_add(ibc.imm) as u32 & ibc.mem_mask) as usize;
                let fsrc = cvt_packed_int_vec_f128(scratchpad, addr);
                let (lo, hi) = nreg.f(ibc.dst);
                *nreg.f_mut(ibc.dst) = (lo - fsrc.0, hi - fsrc.1);
            }
            InstructionType::FscalR => {
                let mask = 0x80F0000000000000u64;
                let (lo, hi) = nreg.f(ibc.dst);
                *nreg.f_mut(ibc.dst) = (
                    f64::from_bits(f64::to_bits(lo) ^ mask),
                    f64::from_bits(f64::to_bits(hi) ^ mask),
                );
            }
            InstructionType::FmulR => {
                let (lo, hi) = nreg.e(ibc.dst);
                let (slo, shi) = nreg.a(ibc.src);
                *nreg.e_mut(ibc.dst) = (lo * slo, hi * shi);
            }
            InstructionType::FdivM => {
                let addr = (nreg.r(ibc.src).wrapping_add(ibc.imm) as u32 & ibc.mem_mask) as usize;
                let fsrc = mask_register_exponent_mantissa(
                    config,
                    cvt_packed_int_vec_f128(scratchpad, addr),
                );
                let (lo, hi) = nreg.e(ibc.dst);
                *nreg.e_mut(ibc.dst) = (lo / fsrc.0, hi / fsrc.1);
            }
            InstructionType::FsqrtR => {
                let (lo, hi) = nreg.e(ibc.dst);
                *nreg.e_mut(ibc.dst) = (lo.sqrt(), hi.sqrt());
            }
            InstructionType::Cbranch => {
                *nreg.r_mut(ibc.dst) = nreg.r(ibc.dst).wrapping_add(ibc.imm);
                if (nreg.r(ibc.dst) & ibc.mem_mask as u64) == 0 {
                    pc = ibc.target as i32;
                }
            }
            InstructionType::Cfround => {
                // V2: conditional — the mode is written only when bits 2-5 of
                // the rotated source are all zero (1/16 chance). Spec §5.4.1.
                let rotated = nreg.r(ibc.src).rotate_right(ibc.imm as u32);
                if version == RxVersion::V1 || (rotated & 60) == 0 {
                    set_rounding_mode((rotated & 3) as u32);
                }
            }
            InstructionType::Istore => {
                let addr = (nreg.r(ibc.dst).wrapping_add(ibc.imm) as u32 & ibc.mem_mask) as usize;
                store64(scratchpad, addr, nreg.r(ibc.src));
            }
            InstructionType::Nop => {}
        }

        pc += 1;
    }
    } // unsafe
}

// ============================================================================
// VM execution for one program chain
// ============================================================================

#[cfg(target_arch = "aarch64")]
#[allow(clippy::too_many_arguments)]
fn execute_vm(
    nreg: &mut NativeRegisterFile,
    scratchpad: &mut [u8],
    program_bytes: &[u8],
    cache_memory: &[u8],
    ss_programs: &[SuperscalarProgram; 8],
    dataset: Option<&RandomXDataset>,
    bytecode_buf: &mut [BytecodeInstruction; RANDOMX_PROGRAM_SIZE_MAX],
    jit: Option<&mut super::jit::JitCompiler>,
    version: RxVersion,
) {
    execute_vm_inner(nreg, scratchpad, program_bytes, cache_memory, ss_programs, dataset, bytecode_buf, jit, version)
}

#[cfg(not(target_arch = "aarch64"))]
fn execute_vm(
    nreg: &mut NativeRegisterFile,
    scratchpad: &mut [u8],
    program_bytes: &[u8],
    cache_memory: &[u8],
    ss_programs: &[SuperscalarProgram; 8],
    dataset: Option<&RandomXDataset>,
    bytecode_buf: &mut [BytecodeInstruction; RANDOMX_PROGRAM_SIZE_MAX],
    version: RxVersion,
) {
    execute_vm_inner(nreg, scratchpad, program_bytes, cache_memory, ss_programs, dataset, bytecode_buf, version)
}

#[allow(clippy::too_many_arguments)]
fn execute_vm_inner(
    nreg: &mut NativeRegisterFile,
    scratchpad: &mut [u8],
    program_bytes: &[u8],
    cache_memory: &[u8],
    ss_programs: &[SuperscalarProgram; 8],
    dataset: Option<&RandomXDataset>,
    bytecode_buf: &mut [BytecodeInstruction; RANDOMX_PROGRAM_SIZE_MAX],
    #[cfg(target_arch = "aarch64")] mut jit: Option<&mut super::jit::JitCompiler>,
    version: RxVersion,
) {
    // NOTE: Rounding mode is NOT reset per-chain. C++ resets once before all chains,
    // and lets CFROUND changes carry over between chains. Caller must call set_rounding_mode(0)
    // once before the chain loop.

    // C++ creates a fresh NativeRegisterFile per execute() call.
    // Reset r-registers to zero; f/e are overwritten each iteration; a is set below.
    nreg.r = [0u64; REGISTERS_COUNT];

    // Initialize from entropy (at the start of program layout)
    let entropy = |idx: usize| -> u64 {
        let off = ENTROPY_OFFSET + idx * 8;
        u64::from_le_bytes(
            program_bytes[off..off + 8]
                .try_into()
                .unwrap(),
        )
    };

    // Initialize a-registers from entropy[0..7]
    for i in 0..REGISTER_COUNT_FLT {
        nreg.a[i] = (
            f64::from_bits(get_small_positive_float_bits(entropy(i * 2))),
            f64::from_bits(get_small_positive_float_bits(entropy(i * 2 + 1))),
        );
    }

    let ma = (entropy(8) as u32) & CACHE_LINE_ALIGN_MASK;
    let mx = entropy(10) as u32;

    let address_registers = entropy(12);
    let read_reg0 = (address_registers & 1) as usize;
    let read_reg1 = 2 + ((address_registers >> 1) & 1) as usize;
    let read_reg2 = 4 + ((address_registers >> 2) & 1) as usize;
    let read_reg3 = 6 + ((address_registers >> 3) & 1) as usize;

    let dataset_offset =
        (entropy(13) % (DATASET_EXTRA_ITEMS + 1)) * CACHE_LINE_SIZE as u64;

    let config = ProgramConfiguration {
        e_mask: [get_float_mask(entropy(14)), get_float_mask(entropy(15))],
        read_reg0,
        read_reg1,
        read_reg2,
        read_reg3,
    };

    // Compile program into pre-allocated buffer
    let program_size = version.program_size();
    let mut register_usage = [0i32; REGISTERS_COUNT];
    compile_program(program_bytes, &mut register_usage, bytecode_buf, program_size);
    let bytecode = &bytecode_buf[..program_size];

    // JIT compile the bytecode to native code (aarch64 only)
    #[cfg(target_arch = "aarch64")]
    let jit_fn = jit.as_mut().map(|jit| {
        jit.compile(bytecode, version);
        unsafe { jit.get_fn() }
    });

    let mut sp_addr0 = mx;
    let mut sp_addr1 = ma;
    let mut mem_mx = mx;
    let mut mem_ma = ma;

    // Main execution loop — all register accesses are unchecked since indices
    // are always valid (read_reg0..3 are 0-7, loop i is 0-7 or 0-3).
    for _ic in 0..RANDOMX_PROGRAM_ITERATIONS {
        unsafe {
        let sp_mix = nreg.r(config.read_reg0) ^ nreg.r(config.read_reg1);
        sp_addr0 ^= sp_mix as u32;
        sp_addr0 &= SCRATCHPAD_L3_MASK64;
        sp_addr1 ^= (sp_mix >> 32) as u32;
        sp_addr1 &= SCRATCHPAD_L3_MASK64;

        // Load r-registers from scratchpad
        for i in 0..REGISTERS_COUNT {
            *nreg.r_mut(i) ^= load64(scratchpad, sp_addr0 as usize + 8 * i);
        }

        // Load f-registers (convert int pairs to float)
        for i in 0..REGISTER_COUNT_FLT {
            *nreg.f_mut(i) = cvt_packed_int_vec_f128(scratchpad, sp_addr1 as usize + 8 * i);
        }

        // Load e-registers (convert int pairs to float, then mask)
        for i in 0..REGISTER_COUNT_FLT {
            *nreg.e_mut(i) = mask_register_exponent_mantissa(
                &config,
                cvt_packed_int_vec_f128(
                    scratchpad,
                    sp_addr1 as usize + 8 * (REGISTER_COUNT_FLT + i),
                ),
            );
        }
        } // unsafe

        // Execute bytecode — JIT on aarch64, interpreter fallback elsewhere
        #[cfg(target_arch = "aarch64")]
        {
            if let Some(f) = jit_fn {
                // JIT: call native code. It reads/writes nreg directly.
                unsafe { f(nreg as *mut NativeRegisterFile, scratchpad.as_mut_ptr(), &config as *const ProgramConfiguration) };
            } else {
                execute_bytecode(bytecode, nreg, scratchpad, &config, version);
            }
        }
        #[cfg(not(target_arch = "aarch64"))]
        execute_bytecode(bytecode, nreg, scratchpad, &config, version);

        // Dataset read
        let read_ptr = dataset_offset + (mem_ma as u64 & CACHE_LINE_ALIGN_MASK as u64);

        unsafe {
        // spMix2 goes into `mp`: mx for V1, ma for V2 (prefetch then runs two
        // iterations ahead of the read). RANDOMX_V2_SEMANTICS.md §4. The read
        // address (`read_ptr`) was captured from the pre-XOR `ma` above.
        let sp_mix2 = (nreg.r(config.read_reg2) ^ nreg.r(config.read_reg3)) as u32;
        match version {
            RxVersion::V1 => mem_mx ^= sp_mix2,
            RxVersion::V2 => mem_ma ^= sp_mix2,
        }

        // Full mode: array lookup. Light mode: compute on-the-fly.
        let item_number = read_ptr / CACHE_LINE_SIZE as u64;
        let dataset_line = match dataset {
            Some(ds) => *ds.get_item(item_number),
            None => init_dataset_item(cache_memory, ss_programs, item_number),
        };
        for i in 0..REGISTERS_COUNT {
            *nreg.r_mut(i) ^= *dataset_line.get_unchecked(i);
        }

        // Swap mx and ma
        std::mem::swap(&mut mem_mx, &mut mem_ma);

        // Prefetch the just-XORed `mp` value (post-swap: mem_ma for V1 — read
        // next iteration; mem_mx for V2 — read the iteration after that).
        // Issued early so the hardware has the full remainder of the iteration to fetch.
        #[cfg(target_arch = "aarch64")]
        if let Some(ds) = dataset {
            let mp = match version {
                RxVersion::V1 => mem_ma,
                RxVersion::V2 => mem_mx,
            };
            let next_read_ptr = dataset_offset + (mp as u64 & CACHE_LINE_ALIGN_MASK as u64);
            let addr = ds.as_ptr().add(next_read_ptr as usize);
            std::arch::asm!("prfm pldl1keep, [{addr}]", addr = in(reg) addr, options(nostack, readonly, preserves_flags));
        }

        // Store r-registers back to scratchpad
        for i in 0..REGISTERS_COUNT {
            store64(scratchpad, sp_addr1 as usize + 8 * i, nreg.r(i));
        }

        // Combine f with e: V1 XORs; V2 runs 4 single AES rounds per f-register
        // with the live e-registers as round keys (RANDOMX_V2_SEMANTICS.md §3).
        match version {
            RxVersion::V1 => {
                for i in 0..REGISTER_COUNT_FLT {
                    *nreg.f_mut(i) = xor_f128(nreg.f(i), nreg.e(i));
                }
            }
            RxVersion::V2 => aes_mix_f_e(nreg),
        }

        // Store f-registers back to scratchpad
        for i in 0..REGISTER_COUNT_FLT {
            store_f128(scratchpad, sp_addr0 as usize + 16 * i, nreg.f(i));
        }

        // Prefetch the next iteration's scratchpad regions.
        // sp_addr0/sp_addr1 reset to 0, so the next values are simply
        // (sp_mix as u32) & MASK and ((sp_mix >> 32) as u32) & MASK
        // where sp_mix uses r-registers AFTER the dataset XOR above.
        #[cfg(target_arch = "aarch64")]
        {
            let next_sp_mix = nreg.r(config.read_reg0) ^ nreg.r(config.read_reg1);
            let next_sp_addr0 = (next_sp_mix as u32 & SCRATCHPAD_L3_MASK64) as usize;
            let next_sp_addr1 = ((next_sp_mix >> 32) as u32 & SCRATCHPAD_L3_MASK64) as usize;
            let base = scratchpad.as_ptr();
            let a0 = base.add(next_sp_addr0);
            let a1 = base.add(next_sp_addr0 + 64);
            let a2 = base.add(next_sp_addr1);
            let a3 = base.add(next_sp_addr1 + 64);
            std::arch::asm!(
                "prfm pldl1keep, [{a0}]",
                "prfm pldl1keep, [{a1}]",
                "prfm pldl1keep, [{a2}]",
                "prfm pldl1keep, [{a3}]",
                a0 = in(reg) a0, a1 = in(reg) a1,
                a2 = in(reg) a2, a3 = in(reg) a3,
                options(nostack, readonly, preserves_flags),
            );
        }

        sp_addr0 = 0;
        sp_addr1 = 0;
        } // unsafe
    }
}

// ============================================================================
// Public API
// ============================================================================

/// RandomX commitment: `blake2b_256(input ‖ hash)` (tevador/RandomX#265).
///
/// For rx/2 (Monero HF v17) the commitment — not the raw RandomX hash — is the
/// value compared against the mining target and submitted as the Stratum
/// `result`; the raw hash travels in the new `commitment` field. `input` must
/// be the exact nonced hashing blob that produced `hash`, byte-for-byte.
/// See RANDOMX_V2_SEMANTICS.md §5.
pub fn calculate_commitment(input: &[u8], hash: &[u8; 32]) -> [u8; 32] {
    let mut buf = Vec::with_capacity(input.len() + 32);
    buf.extend_from_slice(input);
    buf.extend_from_slice(hash);
    blake2b_256(&buf)
}

/// Calculate a RandomX hash (light mode, V1).
/// `key` is the cache key (seed_hash from pool).
/// `input` is the data to hash (block header blob).
/// Returns 32-byte hash.
pub fn calculate_hash(key: &[u8], input: &[u8]) -> [u8; 32] {
    calculate_hash_versioned(key, input, RxVersion::V1)
}

/// Calculate a RandomX v2 (rx/2) hash (light mode).
pub fn calculate_hash_v2(key: &[u8], input: &[u8]) -> [u8; 32] {
    calculate_hash_versioned(key, input, RxVersion::V2)
}

fn calculate_hash_versioned(key: &[u8], input: &[u8], version: RxVersion) -> [u8; 32] {
    // Save and restore FP rounding mode around the entire hash
    let saved_rm = save_rounding_mode();

    // Step 1: Initialize cache from key using Argon2d
    let cache_memory = argon2d_cache(key);

    // Step 2: Generate 8 SuperscalarHash programs from key
    let mut generator = Blake2Generator::new(key, 0);
    let ss_programs: [SuperscalarProgram; 8] = std::array::from_fn(|_| {
        generate_superscalar(&mut generator)
    });

    // Step 3: Blake2b-512(input) -> tempHash (64 bytes)
    let mut temp_hash = blake2b_512(input);

    // Step 4: Fill scratchpad with fillAes1Rx4(tempHash)
    // Note: fillAes1Rx4 modifies temp_hash (AES state carried through)
    let mut scratchpad = vec![0u8; SCRATCHPAD_L3_SIZE];
    fill_aes_1rx4(
        <&mut [u8; 64]>::try_from(&mut temp_hash[..]).unwrap(),
        &mut scratchpad,
    );


    // Initialize native register file
    let mut nreg = NativeRegisterFile::new();

    // Reset rounding mode once before all chains (matches C++ resetRoundingMode)
    // CFROUND changes carry over between chains — do NOT reset per-chain.
    set_rounding_mode(0);

    // Step 5: Execute program chains
    let mut bytecode_buf: Box<[BytecodeInstruction; RANDOMX_PROGRAM_SIZE_MAX]> =
        Box::new(std::array::from_fn(|_| BytecodeInstruction::new()));
    let mut program_bytes = vec![0u8; version.program_bytes_size()];
    for chain in 0..RANDOMX_PROGRAM_COUNT {
        // Generate program from tempHash using fillAes4Rx4
        fill_aes_4rx4(
            <&[u8; 64]>::try_from(&temp_hash[..]).unwrap(),
            &mut program_bytes,
        );

        // Execute VM (light mode)
        execute_vm(
            &mut nreg,
            &mut scratchpad,
            &program_bytes,
            &cache_memory,
            &ss_programs,
            None,
            &mut bytecode_buf,
            #[cfg(target_arch = "aarch64")]
            None,
            version,
        );

        if chain < RANDOMX_PROGRAM_COUNT - 1 {
            // Serialize register file and compute Blake2b-512 for next tempHash
            let reg_bytes = serialize_register_file(&nreg);

            temp_hash = blake2b_512(&reg_bytes);
        }
    }

    // Step 6: Final result
    // hashAes1Rx4(scratchpad) -> overwrite a-registers
    let aes_hash = hash_aes_1rx4(&scratchpad);
    // Write AES hash into the a-registers portion of nreg
    for i in 0..REGISTER_COUNT_FLT {
        let off = i * 16;
        nreg.a[i] = (
            f64::from_bits(u64::from_le_bytes(
                aes_hash[off..off + 8].try_into().unwrap(),
            )),
            f64::from_bits(u64::from_le_bytes(
                aes_hash[off + 8..off + 16].try_into().unwrap(),
            )),
        );
    }

    // Blake2b(registerFile, 256) -> 32-byte output
    let reg_bytes = serialize_register_file(&nreg);
    let result: [u8; 32] = blake2b(32, &reg_bytes).try_into().unwrap();

    restore_rounding_mode(saved_rm);
    result
}

// ============================================================================
// Cached VM for mining (avoids recomputing Argon2d cache per hash)
// ============================================================================

/// A RandomX VM that caches the Argon2d cache and SuperscalarHash programs
/// across multiple hash calculations with the same key.
/// Supports both light mode (no dataset) and full mode (precomputed 2 GiB dataset).
pub struct RandomXVm {
    cache_memory: Vec<u8>,
    ss_programs: [SuperscalarProgram; 8],
    dataset: Option<Arc<RandomXDataset>>,
    scratchpad: Vec<u8>,
    program_bytes: Vec<u8>,
    bytecode: Box<[BytecodeInstruction; RANDOMX_PROGRAM_SIZE_MAX]>,
    version: RxVersion,
    nreg: NativeRegisterFile,
    pipeline_state: [u8; 64],
    #[cfg(target_arch = "aarch64")]
    jit: Option<super::jit::JitCompiler>,
}

impl RandomXVm {
    /// Create a new VM in light mode (256 MiB cache, computes dataset items on-the-fly).
    pub fn new(key: &[u8]) -> Self {
        Self::new_versioned(key, RxVersion::V1)
    }

    /// Light-mode VM for a specific RandomX version.
    pub fn new_versioned(key: &[u8], version: RxVersion) -> Self {
        let cache_memory = argon2d_cache(key);
        let mut generator = Blake2Generator::new(key, 0);
        let ss_programs = std::array::from_fn(|_| generate_superscalar(&mut generator));
        RandomXVm {
            cache_memory,
            ss_programs,
            dataset: None,
            scratchpad: vec![0u8; SCRATCHPAD_L3_SIZE],
            program_bytes: vec![0u8; version.program_bytes_size()],
            bytecode: Box::new(std::array::from_fn(|_| BytecodeInstruction::new())),
            nreg: NativeRegisterFile::new(),
            pipeline_state: [0u8; 64],
            version,
            #[cfg(target_arch = "aarch64")]
            jit: super::jit::JitCompiler::new().ok(),
        }
    }

    /// Create a new VM in full mode with a precomputed dataset.
    pub fn new_full(key: &[u8], dataset: Arc<RandomXDataset>) -> Self {
        Self::new_full_versioned(key, dataset, RxVersion::V1)
    }

    /// Full-mode VM for a specific RandomX version. Dataset contents are
    /// version-independent (same seed -> same dataset for rx/0 and rx/2).
    pub fn new_full_versioned(key: &[u8], dataset: Arc<RandomXDataset>, version: RxVersion) -> Self {
        let cache_memory = argon2d_cache(key);
        let mut generator = Blake2Generator::new(key, 0);
        let ss_programs = std::array::from_fn(|_| generate_superscalar(&mut generator));
        RandomXVm {
            cache_memory,
            ss_programs,
            dataset: Some(dataset),
            scratchpad: vec![0u8; SCRATCHPAD_L3_SIZE],
            program_bytes: vec![0u8; version.program_bytes_size()],
            bytecode: Box::new(std::array::from_fn(|_| BytecodeInstruction::new())),
            nreg: NativeRegisterFile::new(),
            pipeline_state: [0u8; 64],
            version,
            #[cfg(target_arch = "aarch64")]
            jit: super::jit::JitCompiler::new().ok(),
        }
    }

    /// Reinitialize for a new key. Pass `Some(dataset)` for full mode, `None` for light mode.
    pub fn reinit(&mut self, key: &[u8], dataset: Option<Arc<RandomXDataset>>) {
        self.cache_memory = argon2d_cache(key);
        let mut generator = Blake2Generator::new(key, 0);
        self.ss_programs = std::array::from_fn(|_| generate_superscalar(&mut generator));
        self.dataset = dataset;
    }

    /// Get references to cache and programs (for dataset generation).
    pub fn cache_and_programs(&self) -> (&[u8], &[SuperscalarProgram; 8]) {
        (&self.cache_memory, &self.ss_programs)
    }

    /// Calculate a RandomX hash using the cached state.
    /// Reuses pre-allocated scratchpad, program, bytecode, and register file buffers.
    pub fn calculate_hash(&mut self, input: &[u8]) -> [u8; 32] {
        let saved_rm = save_rounding_mode();

        // Blake2b-512(input) -> tempHash
        let mut temp_hash = blake2b_512(input);

        // Fill scratchpad (reuse pre-allocated buffer)
        fill_aes_1rx4(
            <&mut [u8; 64]>::try_from(&mut temp_hash[..]).unwrap(),
            &mut self.scratchpad,
        );

        self.nreg = NativeRegisterFile::new();
        set_rounding_mode(0);

        let ds_ref = self.dataset.as_deref();

        // Execute 8 program chains
        for chain in 0..RANDOMX_PROGRAM_COUNT {
            fill_aes_4rx4(
                <&[u8; 64]>::try_from(&temp_hash[..]).unwrap(),
                &mut self.program_bytes,
            );

            execute_vm(
                &mut self.nreg,
                &mut self.scratchpad,
                &self.program_bytes,
                &self.cache_memory,
                &self.ss_programs,
                ds_ref,
                &mut self.bytecode,
                #[cfg(target_arch = "aarch64")]
                self.jit.as_mut(),
                self.version,
            );

            if chain < RANDOMX_PROGRAM_COUNT - 1 {
                let reg_bytes = serialize_register_file(&self.nreg);
                temp_hash = blake2b_512(&reg_bytes);
            }
        }

        // Final result: hash the scratchpad with AES
        // If we have a next_input, pipeline the hash with the fill for the next hash
        let aes_hash = hash_aes_1rx4(&self.scratchpad);
        for i in 0..REGISTER_COUNT_FLT {
            let off = i * 16;
            self.nreg.a[i] = (
                f64::from_bits(u64::from_le_bytes(
                    aes_hash[off..off + 8].try_into().unwrap(),
                )),
                f64::from_bits(u64::from_le_bytes(
                    aes_hash[off + 8..off + 16].try_into().unwrap(),
                )),
            );
        }

        let reg_bytes = serialize_register_file(&self.nreg);
        let result: [u8; 32] = blake2b(32, &reg_bytes).try_into().unwrap();

        restore_rounding_mode(saved_rm);
        result
    }

    /// Calculate a RandomX hash while simultaneously filling the scratchpad for the next hash.
    /// `next_input` is the blob for the next hash — its Blake2b-512 seeds the fill.
    /// Returns the hash of `input` (the scratchpad must already be filled from a prior call).
    /// Use `prepare_scratchpad` first, then call this in a loop for pipelined mining.
    pub fn calculate_hash_pipelined(&mut self, next_input: &[u8]) -> [u8; 32] {
        let saved_rm = save_rounding_mode();

        self.nreg = NativeRegisterFile::new();
        set_rounding_mode(0);

        let ds_ref = self.dataset.as_deref();

        // temp_hash was set by prepare_scratchpad or previous pipeline step
        let mut temp_hash = self.pipeline_state;

        // Execute 8 program chains
        for chain in 0..RANDOMX_PROGRAM_COUNT {
            fill_aes_4rx4(
                <&[u8; 64]>::try_from(&temp_hash[..]).unwrap(),
                &mut self.program_bytes,
            );

            execute_vm(
                &mut self.nreg,
                &mut self.scratchpad,
                &self.program_bytes,
                &self.cache_memory,
                &self.ss_programs,
                ds_ref,
                &mut self.bytecode,
                #[cfg(target_arch = "aarch64")]
                self.jit.as_mut(),
                self.version,
            );

            if chain < RANDOMX_PROGRAM_COUNT - 1 {
                let reg_bytes = serialize_register_file(&self.nreg);
                temp_hash = blake2b_512(&reg_bytes);
            }
        }

        // Combined hash+fill: hash current scratchpad while filling for next input
        let mut next_temp_hash = blake2b_512(next_input);
        let mut aes_hash = [0u8; 64];
        hash_and_fill_aes_1rx4(
            &mut self.scratchpad,
            &mut aes_hash,
            <&mut [u8; 64]>::try_from(&mut next_temp_hash[..]).unwrap(),
        );
        self.pipeline_state = next_temp_hash;

        for i in 0..REGISTER_COUNT_FLT {
            let off = i * 16;
            self.nreg.a[i] = (
                f64::from_bits(u64::from_le_bytes(
                    aes_hash[off..off + 8].try_into().unwrap(),
                )),
                f64::from_bits(u64::from_le_bytes(
                    aes_hash[off + 8..off + 16].try_into().unwrap(),
                )),
            );
        }

        let reg_bytes = serialize_register_file(&self.nreg);
        let result: [u8; 32] = blake2b(32, &reg_bytes).try_into().unwrap();

        restore_rounding_mode(saved_rm);
        result
    }

    /// Prepare the scratchpad for the first pipelined hash.
    /// Must be called once before the `calculate_hash_pipelined` loop.
    pub fn prepare_scratchpad(&mut self, input: &[u8]) {
        let mut temp_hash = blake2b_512(input);
        fill_aes_1rx4(
            <&mut [u8; 64]>::try_from(&mut temp_hash[..]).unwrap(),
            &mut self.scratchpad,
        );
        self.pipeline_state = temp_hash;
    }

    /// Same as `calculate_hash` but prints timing breakdown for each phase.
    /// Used for profiling only -- call with `cargo test -- --nocapture`.
    #[cfg(test)]
    pub(crate) fn calculate_hash_profiled(&mut self, input: &[u8]) -> [u8; 32] {
        use std::time::Instant;

        let total_start = Instant::now();
        let saved_rm = save_rounding_mode();

        // Phase 1: Blake2b-512 (initial)
        let t = Instant::now();
        let mut temp_hash = blake2b_512(input);
        let blake2b_initial = t.elapsed();

        // Phase 2: fill_aes_1rx4 (scratchpad fill, 2 MiB)
        let t = Instant::now();
        fill_aes_1rx4(
            <&mut [u8; 64]>::try_from(&mut temp_hash[..]).unwrap(),
            &mut self.scratchpad,
        );
        let scratchpad_fill = t.elapsed();

        self.nreg = NativeRegisterFile::new();
        set_rounding_mode(0);

        let ds_ref = self.dataset.as_deref();

        let mut total_fill_aes_4rx4 = std::time::Duration::ZERO;
        let mut total_execute_vm = std::time::Duration::ZERO;
        let mut total_compile = std::time::Duration::ZERO;
        let mut total_blake2b_inter = std::time::Duration::ZERO;

        // Execute 8 program chains
        for chain in 0..RANDOMX_PROGRAM_COUNT {
            // Phase 3: fill_aes_4rx4 (program generation)
            let t = Instant::now();
            fill_aes_4rx4(
                <&[u8; 64]>::try_from(&temp_hash[..]).unwrap(),
                &mut self.program_bytes,
            );
            total_fill_aes_4rx4 += t.elapsed();

            // Phase 4: compile_program (timed separately, extra call)
            let t = Instant::now();
            let mut register_usage = [0i32; REGISTERS_COUNT];
            compile_program(
                &self.program_bytes,
                &mut register_usage,
                &mut self.bytecode,
                self.version.program_size(),
            );
            total_compile += t.elapsed();

            // Phase 5: execute_vm (includes its own compile_program + 2048 iters + dataset)
            let t = Instant::now();
            execute_vm(
                &mut self.nreg,
                &mut self.scratchpad,
                &self.program_bytes,
                &self.cache_memory,
                &self.ss_programs,
                ds_ref,
                &mut self.bytecode,
                #[cfg(target_arch = "aarch64")]
                self.jit.as_mut(),
                self.version,
            );
            total_execute_vm += t.elapsed();

            if chain < RANDOMX_PROGRAM_COUNT - 1 {
                let t = Instant::now();
                let reg_bytes = serialize_register_file(&self.nreg);
                temp_hash = blake2b_512(&reg_bytes);
                total_blake2b_inter += t.elapsed();
            }
        }

        // Phase 6: hash_aes_1rx4 (final scratchpad hash)
        let t = Instant::now();
        let aes_hash = hash_aes_1rx4(&self.scratchpad);
        let hash_aes_final = t.elapsed();

        for i in 0..REGISTER_COUNT_FLT {
            let off = i * 16;
            self.nreg.a[i] = (
                f64::from_bits(u64::from_le_bytes(
                    aes_hash[off..off + 8].try_into().unwrap(),
                )),
                f64::from_bits(u64::from_le_bytes(
                    aes_hash[off + 8..off + 16].try_into().unwrap(),
                )),
            );
        }

        // Phase 7: Blake2b (final)
        let t = Instant::now();
        let reg_bytes = serialize_register_file(&self.nreg);
        let result: [u8; 32] = blake2b(32, &reg_bytes).try_into().unwrap();
        let blake2b_final = t.elapsed();

        restore_rounding_mode(saved_rm);

        let total = total_start.elapsed();

        // compile_program is called separately above AND inside execute_vm,
        // so total_compile shows standalone cost; execute_vm includes its own call.
        println!("\n=== RandomX Hash Profile (light mode) ===");
        println!("  1. blake2b_512 (initial)           {:>10.3?}", blake2b_initial);
        println!("  2. fill_aes_1rx4 (scratchpad 2M)   {:>10.3?}", scratchpad_fill);
        println!("  3. fill_aes_4rx4 (x8 programs)     {:>10.3?}", total_fill_aes_4rx4);
        println!("  4. compile_program (x8, standalone) {:>10.3?}", total_compile);
        println!("  5. execute_vm (x8, incl compile)   {:>10.3?}", total_execute_vm);
        println!("  6. hash_aes_1rx4 (final)           {:>10.3?}", hash_aes_final);
        println!("  7. blake2b_512 (x7 inter-chain)    {:>10.3?}", total_blake2b_inter);
        println!("  8. blake2b (final 32B)             {:>10.3?}", blake2b_final);
        println!("  ─────────────────────────────────────────────");
        println!("  TOTAL                              {:>10.3?}", total);
        println!();
        println!("  Breakdown of execute_vm (x8 chains):");
        println!("    compile_program alone:   {:>10.3?} ({:.1}%)",
            total_compile,
            total_compile.as_secs_f64() / total.as_secs_f64() * 100.0);
        println!("    execute_vm (w/ compile): {:>10.3?} ({:.1}%)",
            total_execute_vm,
            total_execute_vm.as_secs_f64() / total.as_secs_f64() * 100.0);
        let vm_loop_only = total_execute_vm.saturating_sub(total_compile);
        println!("    => VM loop + dataset:    {:>10.3?} ({:.1}%)",
            vm_loop_only,
            vm_loop_only.as_secs_f64() / total.as_secs_f64() * 100.0);
        println!();
        let hash_rate = 1.0 / total.as_secs_f64();
        println!("  Estimated single-thread: {:.1} H/s", hash_rate);
        println!("==========================================\n");

        result
    }
}
