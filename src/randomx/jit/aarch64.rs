// ARM64 instruction encoder for RandomX JIT compiler.
// Each function appends one or more u32 instruction words to a code buffer.
// Reference: ARM Architecture Reference Manual (ARMv8-A)

/// ARM64 register numbers used in the JIT
pub mod reg {
    // Scratch registers (caller-saved)
    pub const X0: u32 = 0;
    pub const X1: u32 = 1;
    pub const X2: u32 = 2;
    pub const X3: u32 = 3;
    pub const X4: u32 = 4;
    pub const X5: u32 = 5;
    pub const X6: u32 = 6;
    pub const X7: u32 = 7;

    // RandomX r[0..7] → x8..x15
    pub const X8: u32 = 8;
    pub const X9: u32 = 9;
    pub const X10: u32 = 10;
    pub const X11: u32 = 11;
    pub const X12: u32 = 12;
    pub const X13: u32 = 13;
    pub const X14: u32 = 14;
    pub const X15: u32 = 15;

    // Scratchpad base pointer
    pub const X16: u32 = 16;
    pub const X17: u32 = 17;
    // NOTE: x18 is reserved on macOS — never use it!

    // Callee-saved, used for e_mask and nreg pointer
    pub const X19: u32 = 19;
    pub const X20: u32 = 20;
    pub const X21: u32 = 21;
    pub const X22: u32 = 22;
    pub const X23: u32 = 23;
    pub const X24: u32 = 24;
    pub const X25: u32 = 25;
    pub const X26: u32 = 26;
    pub const X27: u32 = 27;
    pub const X28: u32 = 28;
    pub const FP: u32 = 29;  // frame pointer
    pub const LR: u32 = 30;  // link register
    pub const SP: u32 = 31;  // stack pointer (or XZR in some contexts)
    pub const XZR: u32 = 31; // zero register

    // SIMD/FP registers
    pub const D0: u32 = 0;
    pub const D1: u32 = 1;
    pub const D2: u32 = 2;
    pub const D3: u32 = 3;
    pub const D4: u32 = 4;
    pub const D5: u32 = 5;
    pub const D6: u32 = 6;
    pub const D7: u32 = 7;
    pub const D8: u32 = 8;
    pub const D9: u32 = 9;
    pub const D10: u32 = 10;
    pub const D11: u32 = 11;
    pub const D12: u32 = 12;
    pub const D13: u32 = 13;
    pub const D14: u32 = 14;
    pub const D15: u32 = 15;
    pub const D16: u32 = 16;
    pub const D17: u32 = 17;
    pub const D18: u32 = 18;
    pub const D19: u32 = 19;
    pub const D20: u32 = 20;
    pub const D21: u32 = 21;
    pub const D22: u32 = 22;
    pub const D23: u32 = 23;
    pub const D24: u32 = 24;
    // d25/d26 are body-clobbered scratch (emit_cvt_packed_int, emit_fswap_r).
    // d27-d31 are unreferenced and genuinely free.
    pub const D25: u32 = 25;
    pub const D26: u32 = 26;
    pub const D27: u32 = 27;
    pub const D28: u32 = 28;
    pub const D29: u32 = 29;
    pub const D30: u32 = 30;
    pub const D31: u32 = 31;
}

/// Code emitter — wraps a Vec<u32> of ARM64 instruction words.
pub struct Emitter {
    pub code: Vec<u32>,
}

impl Default for Emitter {
    fn default() -> Self {
        Self::new()
    }
}

impl Emitter {
    pub fn new() -> Self {
        Emitter {
            code: Vec::with_capacity(4096),
        }
    }

    pub fn len(&self) -> usize {
        self.code.len()
    }

    /// Reset for reuse, keeping the allocated capacity. Lets a `JitCompiler`
    /// emit every program into one buffer instead of allocating per compile.
    #[inline]
    pub fn clear(&mut self) {
        self.code.clear();
    }

    pub fn is_empty(&self) -> bool {
        self.code.is_empty()
    }

    #[inline]
    pub fn emit(&mut self, inst: u32) {
        self.code.push(inst);
    }

    // ========================================================================
    // Integer arithmetic
    // ========================================================================

    /// ADD Xd, Xn, Xm (64-bit)
    pub fn add_reg(&mut self, rd: u32, rn: u32, rm: u32) {
        self.emit(0x8B000000 | (rm << 16) | (rn << 5) | rd);
    }

    /// ADD Xd, Xn, Xm, LSL #shift (64-bit, shift 0-3)
    pub fn add_reg_shifted(&mut self, rd: u32, rn: u32, rm: u32, shift: u32) {
        self.emit(0x8B000000 | (rm << 16) | ((shift & 0x3F) << 10) | (rn << 5) | rd);
    }

    /// ADD Xd, Xn, #imm12 (64-bit, unsigned immediate)
    pub fn add_imm(&mut self, rd: u32, rn: u32, imm12: u32) {
        debug_assert!(imm12 < 4096);
        self.emit(0x91000000 | ((imm12 & 0xFFF) << 10) | (rn << 5) | rd);
    }

    /// SUB Xd, Xn, Xm (64-bit)
    pub fn sub_reg(&mut self, rd: u32, rn: u32, rm: u32) {
        self.emit(0xCB000000 | (rm << 16) | (rn << 5) | rd);
    }

    /// SUB Xd, Xn, #imm12
    pub fn sub_imm(&mut self, rd: u32, rn: u32, imm12: u32) {
        debug_assert!(imm12 < 4096);
        self.emit(0xD1000000 | ((imm12 & 0xFFF) << 10) | (rn << 5) | rd);
    }

    /// SUBS Xd, Xn, #imm12 (64-bit, sets flags).
    /// Distinct from `sub_imm`, which is plain SUB and does NOT set flags —
    /// the native-loop counter needs the flag-setting form.
    pub fn subs_imm(&mut self, rd: u32, rn: u32, imm12: u32) {
        debug_assert!(imm12 < 4096);
        self.emit(0xF1000000 | ((imm12 & 0xFFF) << 10) | (rn << 5) | rd);
    }

    /// EOR Wd, Wn, Wm (32-bit XOR; zeroes bits 63:32 of Xd).
    /// Used for the `ma`/`mx`/`sp_addr` updates, which are `u32` in the Rust
    /// path — a 64-bit EOR would leave the upper half polluted and diverge from
    /// the reference state the differential test compares against.
    pub fn eor_reg_w(&mut self, rd: u32, rn: u32, rm: u32) {
        self.emit(0x4A000000 | (rm << 16) | (rn << 5) | rd);
    }

    /// PRFM PLDL1KEEP, [Xn, Xm] (register offset)
    pub fn prfm_reg(&mut self, rn: u32, rm: u32) {
        self.emit(0xF8A06800 | (rm << 16) | (rn << 5));
    }

    /// PRFM PLDL1KEEP, [Xn, #imm] (unsigned offset, scaled by 8)
    pub fn prfm_imm(&mut self, rn: u32, byte_offset: u32) {
        debug_assert!(byte_offset.is_multiple_of(8));
        let scaled = byte_offset / 8;
        debug_assert!(scaled < 4096);
        self.emit(0xF9800000 | (scaled << 10) | (rn << 5));
    }

    /// STP Dt1, Dt2, [Xn, #imm] (signed offset, scaled by 8, no writeback).
    /// The f-registers are stored as 16 bytes at stride 16; the existing
    /// `stp_fp_pre` mutates the base pointer and is unusable here.
    pub fn stp_fp_imm(&mut self, dt1: u32, dt2: u32, rn: u32, byte_offset: i32) {
        debug_assert!(byte_offset % 8 == 0);
        let imm7 = ((byte_offset / 8) as u32) & 0x7F;
        self.emit(0x6D000000 | (imm7 << 15) | (dt2 << 10) | (rn << 5) | dt1);
    }

    /// LDP Dt1, Dt2, [Xn, #imm] (signed offset, scaled by 8, no writeback)
    pub fn ldp_fp_imm(&mut self, dt1: u32, dt2: u32, rn: u32, byte_offset: i32) {
        debug_assert!(byte_offset % 8 == 0);
        let imm7 = ((byte_offset / 8) as u32) & 0x7F;
        self.emit(0x6D400000 | (imm7 << 15) | (dt2 << 10) | (rn << 5) | dt1);
    }

    /// NEG Xd, Xn (= SUB Xd, XZR, Xn)
    pub fn neg(&mut self, rd: u32, rn: u32) {
        self.sub_reg(rd, reg::XZR, rn);
    }

    /// MUL Xd, Xn, Xm (= MADD Xd, Xn, Xm, XZR)
    pub fn mul(&mut self, rd: u32, rn: u32, rm: u32) {
        self.emit(0x9B007C00 | (rm << 16) | (rn << 5) | rd);
    }

    /// UMULH Xd, Xn, Xm (unsigned multiply high)
    pub fn umulh(&mut self, rd: u32, rn: u32, rm: u32) {
        self.emit(0x9BC07C00 | (rm << 16) | (rn << 5) | rd);
    }

    /// SMULH Xd, Xn, Xm (signed multiply high)
    pub fn smulh(&mut self, rd: u32, rn: u32, rm: u32) {
        self.emit(0x9B407C00 | (rm << 16) | (rn << 5) | rd);
    }

    // ========================================================================
    // Bitwise operations
    // ========================================================================

    /// EOR Xd, Xn, Xm (64-bit XOR)
    pub fn eor_reg(&mut self, rd: u32, rn: u32, rm: u32) {
        self.emit(0xCA000000 | (rm << 16) | (rn << 5) | rd);
    }

    /// AND Xd, Xn, Xm
    pub fn and_reg(&mut self, rd: u32, rn: u32, rm: u32) {
        self.emit(0x8A000000 | (rm << 16) | (rn << 5) | rd);
    }

    /// AND Xd, Xn, #bitmask_imm (logical immediate)
    pub fn and_imm(&mut self, rd: u32, rn: u32, n: u32, immr: u32, imms: u32) {
        self.emit(0x92000000 | (n << 22) | (immr << 16) | (imms << 10) | (rn << 5) | rd);
    }

    /// TST Xn, #bitmask_imm (= ANDS XZR, Xn, #imm)
    pub fn tst_imm(&mut self, rn: u32, n: u32, immr: u32, imms: u32) {
        // ANDS (immediate) 64-bit: 1 11 100100 N immr imms Rn Rd(=XZR)
        self.emit(0xF2000000 | (n << 22) | (immr << 16) | (imms << 10) | (rn << 5) | reg::XZR);
    }

    /// ORR Xd, Xn, Xm
    pub fn orr_reg(&mut self, rd: u32, rn: u32, rm: u32) {
        self.emit(0xAA000000 | (rm << 16) | (rn << 5) | rd);
    }

    // ========================================================================
    // Shifts and rotates
    // ========================================================================

    /// RORV Xd, Xn, Xm (variable rotate right)
    pub fn rorv(&mut self, rd: u32, rn: u32, rm: u32) {
        self.emit(0x9AC02C00 | (rm << 16) | (rn << 5) | rd);
    }

    /// ROR Xd, Xs, #imm6 (= EXTR Xd, Xs, Xs, #imm6)
    pub fn ror_imm(&mut self, rd: u32, rs: u32, imm6: u32) {
        // EXTR (64-bit): 1 00 100111 1 0 Rm imm6 Rn Rd, with Rm=Rn for ROR
        self.emit(0x93C00000 | (rs << 16) | ((imm6 & 0x3F) << 10) | (rs << 5) | rd);
    }

    /// LSR Xd, Xn, Xm (logical shift right variable)
    pub fn lsrv(&mut self, rd: u32, rn: u32, rm: u32) {
        self.emit(0x9AC02400 | (rm << 16) | (rn << 5) | rd);
    }

    /// LSR Xd, Xn, #imm6 (= UBFM Xd, Xn, #imm6, #63)
    pub fn lsr_imm(&mut self, rd: u32, rn: u32, imm6: u32) {
        // UBFM (64-bit): 1 10 100110 1 immr 111111 Rn Rd
        self.emit(0xD340FC00 | ((imm6 & 0x3F) << 16) | (rn << 5) | rd);
    }

    /// LSL Xd, Xn, #imm6 (= UBFM Xd, Xn, #(64-imm6), #(63-imm6))
    pub fn lsl_imm(&mut self, rd: u32, rn: u32, imm6: u32) {
        let immr = (64 - imm6) & 0x3F;
        let imms = (63 - imm6) & 0x3F;
        self.emit(0xD3400000 | (immr << 16) | (imms << 10) | (rn << 5) | rd);
    }

    // ========================================================================
    // Move
    // ========================================================================

    /// MOV Xd, Xn (= ORR Xd, XZR, Xn)
    pub fn mov_reg(&mut self, rd: u32, rn: u32) {
        self.orr_reg(rd, reg::XZR, rn);
    }

    /// MOVZ Xd, #imm16, LSL #shift (shift = 0, 16, 32, 48)
    pub fn movz(&mut self, rd: u32, imm16: u32, shift: u32) {
        let hw = shift / 16;
        self.emit(0xD2800000 | (hw << 21) | ((imm16 & 0xFFFF) << 5) | rd);
    }

    /// MOVK Xd, #imm16, LSL #shift (keep other bits)
    pub fn movk(&mut self, rd: u32, imm16: u32, shift: u32) {
        let hw = shift / 16;
        self.emit(0xF2800000 | (hw << 21) | ((imm16 & 0xFFFF) << 5) | rd);
    }

    /// MOVN Xd, #imm16, LSL #shift (move wide with NOT)
    pub fn movn(&mut self, rd: u32, imm16: u32, shift: u32) {
        let hw = shift / 16;
        self.emit(0x92800000 | (hw << 21) | ((imm16 & 0xFFFF) << 5) | rd);
    }

    /// Load a 64-bit immediate into register using MOVZ + MOVK chain.
    /// Optimizes for small values.
    pub fn mov_imm64(&mut self, rd: u32, imm: u64) {
        if imm == 0 {
            self.movz(rd, 0, 0);
            return;
        }

        // Check if it can be encoded as MOVN (all-ones pattern with one chunk different)
        let inv = !imm;
        let chunks = [
            (imm & 0xFFFF) as u32,
            ((imm >> 16) & 0xFFFF) as u32,
            ((imm >> 32) & 0xFFFF) as u32,
            ((imm >> 48) & 0xFFFF) as u32,
        ];
        let inv_chunks = [
            (inv & 0xFFFF) as u32,
            ((inv >> 16) & 0xFFFF) as u32,
            ((inv >> 32) & 0xFFFF) as u32,
            ((inv >> 48) & 0xFFFF) as u32,
        ];

        // Count non-zero chunks
        let nz_count = chunks.iter().filter(|&&c| c != 0).count();
        let inv_nz_count = inv_chunks.iter().filter(|&&c| c != 0).count();

        if inv_nz_count < nz_count {
            // Use MOVN + MOVK
            let mut first = true;
            for i in 0..4 {
                if inv_chunks[i] != 0 || first {
                    if first {
                        // Find first chunk where inv is non-zero (or use 0)
                        let idx = (0..4).find(|&j| inv_chunks[j] != 0).unwrap_or(0);
                        self.movn(rd, inv_chunks[idx], idx as u32 * 16);
                        first = false;
                        if i < idx { continue; }
                        if i == idx { continue; }
                    }
                    if chunks[i] != 0xFFFF {
                        self.movk(rd, chunks[i], i as u32 * 16);
                    }
                }
            }
            // Simpler approach: MOVN with first non-zero inv chunk, then MOVK for rest
        } else {
            // Use MOVZ + MOVK
            let mut first = true;
            for i in 0..4 {
                if chunks[i] != 0 || (first && i == 3) {
                    if first {
                        self.movz(rd, chunks[i], i as u32 * 16);
                        first = false;
                    } else {
                        self.movk(rd, chunks[i], i as u32 * 16);
                    }
                }
            }
        }
    }

    // ========================================================================
    // Memory load/store
    // ========================================================================

    /// LDR Xd, [Xn, Xm] (register offset, 64-bit, LSL #0)
    pub fn ldr_reg(&mut self, rt: u32, rn: u32, rm: u32) {
        // LDR (register): 11 111 000 01 1 Rm 011 0 10 Rn Rt (option=011=LSL, S=0)
        self.emit(0xF8606800 | (rm << 16) | (rn << 5) | rt);
    }

    /// LDR Xd, [Xn, #imm12*8] (unsigned offset, 64-bit, scaled)
    pub fn ldr_imm(&mut self, rt: u32, rn: u32, imm_byte_offset: u32) {
        debug_assert!(imm_byte_offset.is_multiple_of(8));
        let scaled = imm_byte_offset / 8;
        debug_assert!(scaled < 4096);
        self.emit(0xF9400000 | (scaled << 10) | (rn << 5) | rt);
    }

    /// STR Xd, [Xn, Xm] (register offset, 64-bit, LSL #0)
    pub fn str_reg(&mut self, rt: u32, rn: u32, rm: u32) {
        // STR (register): 11 111 000 00 1 Rm 011 0 10 Rn Rt (option=011=LSL, S=0)
        self.emit(0xF8206800 | (rm << 16) | (rn << 5) | rt);
    }

    /// STR Xd, [Xn, #imm12*8] (unsigned offset, 64-bit, scaled)
    pub fn str_imm(&mut self, rt: u32, rn: u32, imm_byte_offset: u32) {
        debug_assert!(imm_byte_offset.is_multiple_of(8));
        let scaled = imm_byte_offset / 8;
        debug_assert!(scaled < 4096);
        self.emit(0xF9000000 | (scaled << 10) | (rn << 5) | rt);
    }

    /// LDR Dd, [Xn, #imm12*8] (FP/SIMD 64-bit load, unsigned offset)
    pub fn ldr_fp_imm(&mut self, dt: u32, rn: u32, imm_byte_offset: u32) {
        debug_assert!(imm_byte_offset.is_multiple_of(8));
        let scaled = imm_byte_offset / 8;
        debug_assert!(scaled < 4096);
        self.emit(0xFD400000 | (scaled << 10) | (rn << 5) | dt);
    }

    /// STR Dd, [Xn, #imm12*8] (FP/SIMD 64-bit store, unsigned offset)
    pub fn str_fp_imm(&mut self, dt: u32, rn: u32, imm_byte_offset: u32) {
        debug_assert!(imm_byte_offset.is_multiple_of(8));
        let scaled = imm_byte_offset / 8;
        debug_assert!(scaled < 4096);
        self.emit(0xFD000000 | (scaled << 10) | (rn << 5) | dt);
    }

    /// STP Xt1, Xt2, [Xn, #imm7*8]! (pre-index store pair, 64-bit)
    pub fn stp_pre(&mut self, rt1: u32, rt2: u32, rn: u32, imm_byte_offset: i32) {
        let imm7 = ((imm_byte_offset / 8) as u32) & 0x7F;
        self.emit(0xA9800000 | (imm7 << 15) | (rt2 << 10) | (rn << 5) | rt1);
    }

    /// LDP Xt1, Xt2, [Xn], #imm7*8 (post-index load pair, 64-bit)
    pub fn ldp_post(&mut self, rt1: u32, rt2: u32, rn: u32, imm_byte_offset: i32) {
        let imm7 = ((imm_byte_offset / 8) as u32) & 0x7F;
        self.emit(0xA8C00000 | (imm7 << 15) | (rt2 << 10) | (rn << 5) | rt1);
    }

    /// STP Dt1, Dt2, [Xn, #imm7*8]! (pre-index store pair, FP 64-bit)
    pub fn stp_fp_pre(&mut self, dt1: u32, dt2: u32, rn: u32, imm_byte_offset: i32) {
        let imm7 = ((imm_byte_offset / 8) as u32) & 0x7F;
        self.emit(0x6D800000 | (imm7 << 15) | (dt2 << 10) | (rn << 5) | dt1);
    }

    /// LDP Dt1, Dt2, [Xn], #imm7*8 (post-index load pair, FP 64-bit)
    pub fn ldp_fp_post(&mut self, dt1: u32, dt2: u32, rn: u32, imm_byte_offset: i32) {
        let imm7 = ((imm_byte_offset / 8) as u32) & 0x7F;
        self.emit(0x6CC00000 | (imm7 << 15) | (dt2 << 10) | (rn << 5) | dt1);
    }

    /// LDR Wd, [Xn, Xm] (32-bit load, register offset, LSL #0)
    pub fn ldr_w_reg(&mut self, rt: u32, rn: u32, rm: u32) {
        self.emit(0xB8606800 | (rm << 16) | (rn << 5) | rt);
    }

    /// LDRSW Xd, [Xn, Xm] (sign-extend 32-bit to 64-bit, register offset, LSL #0)
    pub fn ldrsw_reg(&mut self, rt: u32, rn: u32, rm: u32) {
        self.emit(0xB8A06800 | (rm << 16) | (rn << 5) | rt);
    }

    // ========================================================================
    // Floating point operations
    // ========================================================================

    /// FADD Dd, Dn, Dm (double-precision add)
    pub fn fadd(&mut self, rd: u32, rn: u32, rm: u32) {
        self.emit(0x1E602800 | (rm << 16) | (rn << 5) | rd);
    }

    /// FSUB Dd, Dn, Dm (double-precision sub)
    pub fn fsub(&mut self, rd: u32, rn: u32, rm: u32) {
        self.emit(0x1E603800 | (rm << 16) | (rn << 5) | rd);
    }

    /// FMUL Dd, Dn, Dm (double-precision mul)
    pub fn fmul(&mut self, rd: u32, rn: u32, rm: u32) {
        self.emit(0x1E600800 | (rm << 16) | (rn << 5) | rd);
    }

    /// FDIV Dd, Dn, Dm (double-precision div)
    pub fn fdiv(&mut self, rd: u32, rn: u32, rm: u32) {
        self.emit(0x1E601800 | (rm << 16) | (rn << 5) | rd);
    }

    /// FSQRT Dd, Dn (double-precision square root)
    pub fn fsqrt(&mut self, rd: u32, rn: u32) {
        self.emit(0x1E61C000 | (rn << 5) | rd);
    }

    /// FMOV Dd, Dn (FP register to FP register, double)
    pub fn fmov_dd(&mut self, rd: u32, rn: u32) {
        self.emit(0x1E604000 | (rn << 5) | rd);
    }

    /// FMOV Xd, Dn (FP register to general register, 64-bit)
    pub fn fmov_xd(&mut self, rd: u32, dn: u32) {
        // FMOV (FP to GP): 1 00 11110 01 1 00110 000000 Rn Rd
        self.emit(0x9E660000 | (dn << 5) | rd);
    }

    /// FMOV Dd, Xn (general register to FP register, 64-bit)
    pub fn fmov_dx(&mut self, dd: u32, rn: u32) {
        // FMOV (GP to FP): 1 00 11110 01 1 00111 000000 Rn Rd
        self.emit(0x9E670000 | (rn << 5) | dd);
    }

    /// SCVTF Dd, Xn (signed 64-bit integer to double)
    pub fn scvtf_dx(&mut self, dd: u32, xn: u32) {
        self.emit(0x9E620000 | (xn << 5) | dd);
    }

    /// SCVTF Dd, Wn (signed 32-bit integer to double)
    pub fn scvtf_dw(&mut self, dd: u32, wn: u32) {
        self.emit(0x1E620000 | (wn << 5) | dd);
    }

    /// EOR Vd.8B, Vn.8B, Vm.8B (SIMD XOR on 8-byte vectors — used for FSCAL)
    pub fn eor_v8b(&mut self, vd: u32, vn: u32, vm: u32) {
        self.emit(0x2E201C00 | (vm << 16) | (vn << 5) | vd);
    }

    // ========================================================================
    // Branching
    // ========================================================================

    /// B.cond (conditional branch, offset in words from current PC)
    /// cond: 0=EQ, 1=NE, 2=CS, 3=CC, ...
    pub fn b_cond(&mut self, cond: u32, offset_words: i32) {
        let imm19 = (offset_words as u32) & 0x7FFFF;
        self.emit(0x54000000 | (imm19 << 5) | cond);
    }

    /// B (unconditional branch, offset in words)
    pub fn b(&mut self, offset_words: i32) {
        let imm26 = (offset_words as u32) & 0x3FFFFFF;
        self.emit(0x14000000 | imm26);
    }

    /// BL (branch with link)
    pub fn bl(&mut self, offset_words: i32) {
        let imm26 = (offset_words as u32) & 0x3FFFFFF;
        self.emit(0x94000000 | imm26);
    }

    /// RET (return via x30/LR)
    pub fn ret(&mut self) {
        self.emit(0xD65F03C0);
    }

    /// BR Xn (branch to register)
    pub fn br(&mut self, rn: u32) {
        self.emit(0xD61F0000 | (rn << 5));
    }

    // ========================================================================
    // System registers
    // ========================================================================

    /// MSR FPCR, Xn (write floating-point control register)
    pub fn msr_fpcr(&mut self, rn: u32) {
        // MSR FPCR: 1101 0101 0001 0100 0100 0100 001 Rt
        self.emit(0xD51B4400 | rn);
    }

    /// MRS Xd, FPCR (read floating-point control register)
    pub fn mrs_fpcr(&mut self, rd: u32) {
        self.emit(0xD53B4400 | rd);
    }

    // ========================================================================
    // Bitmask immediate encoding
    // ========================================================================

    /// Encode a bitmask immediate for 64-bit logical instructions.
    /// Returns (N, immr, imms) or None if not encodable.
    pub fn encode_bitmask_imm(value: u64) -> Option<(u32, u32, u32)> {
        if value == 0 || value == !0u64 {
            return None;
        }

        // Determine the smallest repeating element size
        let mut size = 64u32;
        let mut tmp = value;

        while size > 2 {
            let half = size / 2;
            let mask = if half >= 64 { !0u64 } else { (1u64 << half) - 1 };
            if (tmp & mask) == ((tmp >> half) & mask) {
                size = half;
                tmp &= mask;
            } else {
                break;
            }
        }

        // Extract the pattern for this element size
        let mask = if size >= 64 { !0u64 } else { (1u64 << size) - 1 };
        let pattern = value & mask;

        // Find rotation that makes the pattern a contiguous run of 1s
        let mut ones = 0u32;
        let mut rotation = 0u32;

        for rot in 0..size {
            let rotated = if rot == 0 {
                pattern
            } else {
                ((pattern >> rot) | (pattern << (size - rot))) & mask
            };
            // Count trailing ones
            let trail = rotated.trailing_ones().min(size);
            // Check if it's a contiguous run (all ones are at the bottom)
            let run_mask = if trail >= 64 { !0u64 } else { (1u64 << trail) - 1 };
            if rotated == (run_mask & mask) && trail > 0 {
                ones = trail;
                rotation = rot;
                break;
            }
        }

        if ones == 0 {
            return None;
        }

        let n = if size == 64 { 1u32 } else { 0u32 };
        // immr: the mask is created as `ones` 1-bits at bit 0, then rotated RIGHT by immr.
        // We found that rotating right by `rotation` aligns the pattern to bit 0.
        // So immr = (size - rotation) % size to rotate back.
        let immr = if rotation == 0 { 0 } else { (size - rotation) & (size - 1) };
        // imms encodes both the element size and the number of ones
        let size_encoding = (!(size * 2 - 1)) & 0x3F;
        let imms = size_encoding | (ones - 1);

        Some((n, immr, imms & 0x3F))
    }

    /// AND Xd, Xn, #value using bitmask immediate encoding.
    /// Falls back to loading into tmp register if not encodable.
    pub fn and_bitmask(&mut self, rd: u32, rn: u32, value: u64, tmp: u32) {
        if let Some((n, immr, imms)) = Self::encode_bitmask_imm(value) {
            self.and_imm(rd, rn, n, immr, imms);
        } else {
            self.mov_imm64(tmp, value);
            self.and_reg(rd, rn, tmp);
        }
    }

    /// TST Xn, #value using bitmask immediate encoding.
    /// Falls back to loading into tmp register if not encodable.
    pub fn tst_bitmask(&mut self, rn: u32, value: u64, tmp: u32) {
        if let Some((n, immr, imms)) = Self::encode_bitmask_imm(value) {
            self.tst_imm(rn, n, immr, imms);
        } else {
            self.mov_imm64(tmp, value);
            // TST Xn, Xm = ANDS XZR, Xn, Xm
            self.emit(0xEA000000 | (tmp << 16) | (rn << 5) | reg::XZR);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_add_reg() {
        let mut e = Emitter::new();
        e.add_reg(reg::X8, reg::X9, reg::X10);
        assert_eq!(e.code[0], 0x8B0A0128);
    }

    #[test]
    fn test_sub_reg() {
        let mut e = Emitter::new();
        e.sub_reg(reg::X8, reg::X9, reg::X10);
        assert_eq!(e.code[0], 0xCB0A0128);
    }

    #[test]
    fn test_mul() {
        let mut e = Emitter::new();
        e.mul(reg::X8, reg::X9, reg::X10);
        assert_eq!(e.code[0], 0x9B0A7D28);
    }

    #[test]
    fn test_umulh() {
        let mut e = Emitter::new();
        e.umulh(reg::X8, reg::X9, reg::X10);
        assert_eq!(e.code[0], 0x9BCA7D28);
    }

    #[test]
    fn test_smulh() {
        let mut e = Emitter::new();
        e.smulh(reg::X8, reg::X9, reg::X10);
        assert_eq!(e.code[0], 0x9B4A7D28);
    }

    #[test]
    fn test_eor_reg() {
        let mut e = Emitter::new();
        e.eor_reg(reg::X8, reg::X9, reg::X10);
        assert_eq!(e.code[0], 0xCA0A0128);
    }

    #[test]
    fn test_neg() {
        let mut e = Emitter::new();
        e.neg(reg::X8, reg::X9);
        // NEG X8, X9 = SUB X8, XZR, X9
        assert_eq!(e.code[0], 0xCB0903E8);
    }

    #[test]
    fn test_mov_reg() {
        let mut e = Emitter::new();
        e.mov_reg(reg::X8, reg::X9);
        // MOV X8, X9 = ORR X8, XZR, X9
        assert_eq!(e.code[0], 0xAA0903E8);
    }

    #[test]
    fn test_movz() {
        let mut e = Emitter::new();
        e.movz(reg::X0, 42, 0);
        assert_eq!(e.code[0], 0xD2800540);
    }

    #[test]
    fn test_ret() {
        let mut e = Emitter::new();
        e.ret();
        assert_eq!(e.code[0], 0xD65F03C0);
    }

    #[test]
    fn test_ldr_imm() {
        let mut e = Emitter::new();
        // LDR X8, [X21, #0] (offset 0)
        e.ldr_imm(reg::X8, reg::X21, 0);
        assert_eq!(e.code[0], 0xF94002A8);
    }

    #[test]
    fn test_str_imm() {
        let mut e = Emitter::new();
        // STR X8, [X21, #8]
        e.str_imm(reg::X8, reg::X21, 8);
        assert_eq!(e.code[0], 0xF90006A8);
    }

    #[test]
    fn test_fadd() {
        let mut e = Emitter::new();
        e.fadd(reg::D0, reg::D1, reg::D2);
        assert_eq!(e.code[0], 0x1E622820);
    }

    #[test]
    fn test_fsqrt() {
        let mut e = Emitter::new();
        e.fsqrt(reg::D0, reg::D1);
        assert_eq!(e.code[0], 0x1E61C020);
    }

    #[test]
    fn test_b_cond_eq() {
        let mut e = Emitter::new();
        // B.EQ +4 (1 word forward)
        e.b_cond(0, 1);
        assert_eq!(e.code[0], 0x54000020);
    }

    #[test]
    fn test_mov_imm64_small() {
        let mut e = Emitter::new();
        e.mov_imm64(reg::X0, 0x1234);
        assert_eq!(e.code.len(), 1); // Single MOVZ
    }

    #[test]
    fn test_mov_imm64_two_chunks() {
        let mut e = Emitter::new();
        e.mov_imm64(reg::X0, 0x1234_5678);
        assert_eq!(e.code.len(), 2); // MOVZ + MOVK
    }

    #[test]
    fn test_bitmask_imm_power_of_2_minus_1() {
        // 0x1FFF = 13 ones = valid bitmask immediate
        let result = Emitter::encode_bitmask_imm(0x1FFF);
        assert!(result.is_some());
    }

    #[test]
    fn test_bitmask_imm_scratchpad_masks() {
        // SCRATCHPAD_L1_MASK = 0x3FF8 (byte offset mask for 16KB scratchpad)
        let result = Emitter::encode_bitmask_imm(0x3FF8);
        assert!(result.is_some(), "L1 mask must be encodable as bitmask immediate");

        // SCRATCHPAD_L2_MASK = 0x3FFF8
        let result = Emitter::encode_bitmask_imm(0x3FFF8);
        assert!(result.is_some(), "L2 mask must be encodable as bitmask immediate");

        // SCRATCHPAD_L3_MASK = 0x1FFFF8
        let result = Emitter::encode_bitmask_imm(0x1FFFF8);
        assert!(result.is_some(), "L3 mask must be encodable as bitmask immediate");

        // Masks the native loop additionally needs (DESIGN_JIT_NATIVE_LOOP.md
        // C1/C9). If either stopped being encodable the address computation
        // would need a temp register it does not have.
        assert!(
            Emitter::encode_bitmask_imm(0x1FFFC0).is_some(),
            "SCRATCHPAD_L3_MASK64 must be encodable as bitmask immediate"
        );
        // CACHE_LINE_ALIGN_MASK. This one is the memory-safety bound on the
        // JIT dataset read: the native loop has no bounds check, and the
        // worst-case read ends one cache line short of DATASET_TOTAL_SIZE.
        assert!(
            Emitter::encode_bitmask_imm(0x7FFFFFC0).is_some(),
            "CACHE_LINE_ALIGN_MASK must be encodable as bitmask immediate"
        );
    }

    #[test]
    fn test_ror_imm() {
        let mut e = Emitter::new();
        // ROR X8, X9, #5 = EXTR X8, X9, X9, #5
        e.ror_imm(reg::X8, reg::X9, 5);
        // 1001 0011 1100 0000 0 (imm6=000101) (Rn=01001) (Rd=01000)
        // = 0x93C91528
        let expected = 0x93C00000 | (reg::X9 << 16) | (5 << 10) | (reg::X9 << 5) | reg::X8;
        assert_eq!(e.code[0], expected);
    }

    #[test]
    fn test_stp_pre() {
        let mut e = Emitter::new();
        // STP X29, X30, [SP, #-16]!
        e.stp_pre(reg::FP, reg::LR, reg::SP, -16);
        // imm7 = -16/8 = -2 = 0x7E
        assert_eq!(e.code[0], 0xA9BF7BFD);
    }

    #[test]
    fn test_ldp_post() {
        let mut e = Emitter::new();
        // LDP X29, X30, [SP], #16
        e.ldp_post(reg::FP, reg::LR, reg::SP, 16);
        assert_eq!(e.code[0], 0xA8C17BFD);
    }

    #[test]
    fn test_eor_v8b() {
        let mut e = Emitter::new();
        // EOR V0.8B, V1.8B, V24.8B
        e.eor_v8b(reg::D0, reg::D1, reg::D24);
        assert_eq!(e.code[0], 0x2E381C20);
    }

    #[test]
    fn test_add_reg_shifted() {
        let mut e = Emitter::new();
        // ADD X8, X9, X10, LSL #2
        e.add_reg_shifted(reg::X8, reg::X9, reg::X10, 2);
        let expected = 0x8B000000 | (reg::X10 << 16) | (2 << 10) | (reg::X9 << 5) | reg::X8;
        assert_eq!(e.code[0], expected);
    }

    // ---- native-loop encoders (DESIGN_JIT_NATIVE_LOOP.md C7) ----
    // Every expected word below was produced by `as -arch arm64`.

    #[test]
    fn test_subs_imm_sets_flags() {
        // subs x28, x28, #1 -> 0xF100079C. Distinct from sub_imm (0xD1...),
        // which does NOT set flags and cannot drive the loop counter.
        let mut e = Emitter::new();
        e.subs_imm(reg::X28, reg::X28, 1);
        assert_eq!(e.code[0], 0xF100079C);
        let mut e = Emitter::new();
        e.subs_imm(reg::X0, reg::X1, 4095);
        assert_eq!(e.code[0], 0xF13FFC20);
        // guard against a regression to the non-flag-setting form
        let mut e = Emitter::new();
        e.sub_imm(reg::X28, reg::X28, 1);
        assert_ne!(e.code[0], 0xF100079C, "sub_imm must not set flags");
    }

    #[test]
    fn test_eor_reg_w() {
        // eor w25, w25, w0 -> 0x4A000339 (32-bit; zeroes bits 63:32)
        let mut e = Emitter::new();
        e.eor_reg_w(reg::X25, reg::X25, reg::X0);
        assert_eq!(e.code[0], 0x4A000339);
    }

    #[test]
    fn test_prfm() {
        let mut e = Emitter::new();
        e.prfm_reg(reg::X22, reg::X24);
        assert_eq!(e.code[0], 0xF8B86AC0); // prfm pldl1keep, [x22, x24]
        let mut e = Emitter::new();
        e.prfm_imm(reg::X16, 64);
        assert_eq!(e.code[0], 0xF9802200); // prfm pldl1keep, [x16, #64]
        let mut e = Emitter::new();
        e.prfm_imm(reg::X16, 0);
        assert_eq!(e.code[0], 0xF9800200); // prfm pldl1keep, [x16]
    }

    #[test]
    fn test_stp_ldp_fp_imm() {
        let mut e = Emitter::new();
        e.stp_fp_imm(reg::D0, reg::D1, reg::X0, 0);
        assert_eq!(e.code[0], 0x6D000400); // stp d0, d1, [x0]
        let mut e = Emitter::new();
        e.stp_fp_imm(reg::D2, reg::D3, reg::X0, 16);
        assert_eq!(e.code[0], 0x6D010C02); // stp d2, d3, [x0, #16]
        let mut e = Emitter::new();
        e.ldp_fp_imm(reg::D0, reg::D1, reg::X0, 32);
        assert_eq!(e.code[0], 0x6D420400); // ldp d0, d1, [x0, #32]
    }
}
