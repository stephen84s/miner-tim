// AES-based generator and hash functions
// Reference: RandomX src/aes_hash.cpp
//
// Uses hardware AES intrinsics (AES-NI on x86_64, NEON crypto on aarch64)
// when available, falling back to software T-table implementation.
// SIMD versions keep all 4 states in registers across the entire fill/hash loop.

use super::soft_aes::{soft_aesdec, soft_aesenc};

/// Convert 4 u32 values to a 16-byte array matching C++ rx_set_int_vec_i128 / _mm_set_epi32.
/// _mm_set_epi32(a, b, c, d) stores as [d_le, c_le, b_le, a_le] in memory.
const fn key_from_u32s(a: u32, b: u32, c: u32, d: u32) -> [u8; 16] {
    let a = a.to_le_bytes();
    let b = b.to_le_bytes();
    let c = c.to_le_bytes();
    let d = d.to_le_bytes();
    [
        d[0], d[1], d[2], d[3], c[0], c[1], c[2], c[3], b[0], b[1], b[2], b[3], a[0], a[1],
        a[2], a[3],
    ]
}

// AesGenerator1R keys
pub(super) const GEN_1R_KEY0: [u8; 16] = key_from_u32s(0xb4f44917, 0xdbb5552b, 0x62716609, 0x6daca553);
pub(super) const GEN_1R_KEY1: [u8; 16] = key_from_u32s(0x0da1dc4e, 0x1725d378, 0x846a710d, 0x6d7caf07);
pub(super) const GEN_1R_KEY2: [u8; 16] = key_from_u32s(0x3e20e345, 0xf4c0794f, 0x9f947ec6, 0x3f1262f1);
pub(super) const GEN_1R_KEY3: [u8; 16] = key_from_u32s(0x49169154, 0x16314c88, 0xb1ba317c, 0x6aef8135);

// AesGenerator4R keys
const GEN_4R_KEY0: [u8; 16] = key_from_u32s(0x99e5d23f, 0x2f546d2b, 0xd1833ddb, 0x6421aadd);
const GEN_4R_KEY1: [u8; 16] = key_from_u32s(0xa5dfcde5, 0x06f79d53, 0xb6913f55, 0xb20e3450);
const GEN_4R_KEY2: [u8; 16] = key_from_u32s(0x171c02bf, 0x0aa4679f, 0x515e7baf, 0x5c3ed904);
const GEN_4R_KEY3: [u8; 16] = key_from_u32s(0xd8ded291, 0xcd673785, 0xe78f5d08, 0x85623763);
const GEN_4R_KEY4: [u8; 16] = key_from_u32s(0x229effb4, 0x3d518b6d, 0xe3d6a7a6, 0xb5826f73);
const GEN_4R_KEY5: [u8; 16] = key_from_u32s(0xb272b7d2, 0xe9024d4e, 0x9c10b3d9, 0xc7566bf3);
const GEN_4R_KEY6: [u8; 16] = key_from_u32s(0xf63befa7, 0x2ba9660a, 0xf765a38b, 0xf273c9e7);
const GEN_4R_KEY7: [u8; 16] = key_from_u32s(0xc0b0762d, 0x0c06d1fd, 0x915839de, 0x7a7cd609);

// AesHash1R initial state
pub(super) const HASH_1R_STATE0: [u8; 16] = key_from_u32s(0xd7983aad, 0xcc82db47, 0x9fa856de, 0x92b52c0d);
pub(super) const HASH_1R_STATE1: [u8; 16] = key_from_u32s(0xace78057, 0xf59e125a, 0x15c7b798, 0x338d996e);
pub(super) const HASH_1R_STATE2: [u8; 16] = key_from_u32s(0xe8a07ce4, 0x5079506b, 0xae62c7d0, 0x6a770017);
pub(super) const HASH_1R_STATE3: [u8; 16] = key_from_u32s(0x7e994948, 0x79a10005, 0x07ad828d, 0x630a240c);

// AesHash1R extra keys
pub(super) const HASH_1R_XKEY0: [u8; 16] = key_from_u32s(0x06890201, 0x90dc56bf, 0x8b24949f, 0xf6fa8389);
pub(super) const HASH_1R_XKEY1: [u8; 16] = key_from_u32s(0xed18f99b, 0xee1043c6, 0x51f4e03c, 0x61b263d1);

fn load_block(data: &[u8], offset: usize) -> [u8; 16] {
    data[offset..offset + 16].try_into().unwrap()
}

fn store_block(data: &mut [u8], offset: usize, block: &[u8; 16]) {
    data[offset..offset + 16].copy_from_slice(block);
}

// ============================================================================
// Public API — dispatches to SIMD or software
// ============================================================================

pub fn fill_aes_1rx4(state: &mut [u8; 64], output: &mut [u8]) {
    assert!(output.len().is_multiple_of(64));
    #[cfg(target_arch = "aarch64")]
    {
        if std::arch::is_aarch64_feature_detected!("aes") {
            unsafe { fill_aes_1rx4_neon(state, output) };
            return;
        }
    }
    #[cfg(target_arch = "x86_64")]
    {
        if std::arch::is_x86_feature_detected!("aes") {
            unsafe { fill_aes_1rx4_aesni(state, output) };
            return;
        }
    }
    fill_aes_1rx4_soft(state, output);
}

pub fn fill_aes_4rx4(state: &[u8; 64], output: &mut [u8]) {
    assert!(output.len().is_multiple_of(64));
    #[cfg(target_arch = "aarch64")]
    {
        if std::arch::is_aarch64_feature_detected!("aes") {
            unsafe { fill_aes_4rx4_neon(state, output) };
            return;
        }
    }
    #[cfg(target_arch = "x86_64")]
    {
        if std::arch::is_x86_feature_detected!("aes") {
            unsafe { fill_aes_4rx4_aesni(state, output) };
            return;
        }
    }
    fill_aes_4rx4_soft(state, output);
}

pub fn hash_aes_1rx4(input: &[u8]) -> [u8; 64] {
    assert!(input.len().is_multiple_of(64));
    #[cfg(target_arch = "aarch64")]
    {
        if std::arch::is_aarch64_feature_detected!("aes") {
            return unsafe { hash_aes_1rx4_neon(input) };
        }
    }
    #[cfg(target_arch = "x86_64")]
    {
        if std::arch::is_x86_feature_detected!("aes") {
            return unsafe { hash_aes_1rx4_aesni(input) };
        }
    }
    hash_aes_1rx4_soft(input)
}

/// Combined hash-and-fill: hashes scratchpad into 64-byte hash_out while
/// simultaneously refilling the scratchpad using fill_state.
/// Matches the C++ hashAndFillAes1Rx4 function.
pub fn hash_and_fill_aes_1rx4(scratchpad: &mut [u8], hash_out: &mut [u8; 64], fill_state: &mut [u8; 64]) {
    assert!(scratchpad.len().is_multiple_of(64));
    #[cfg(target_arch = "aarch64")]
    {
        if std::arch::is_aarch64_feature_detected!("aes") {
            unsafe { hash_and_fill_aes_1rx4_neon(scratchpad, hash_out, fill_state) };
            return;
        }
    }
    #[cfg(target_arch = "x86_64")]
    {
        if std::arch::is_x86_feature_detected!("aes") {
            unsafe { hash_and_fill_aes_1rx4_aesni(scratchpad, hash_out, fill_state) };
            return;
        }
    }
    hash_and_fill_aes_1rx4_soft(scratchpad, hash_out, fill_state);
}

// ============================================================================
// Software fallback (T-table based)
// ============================================================================

fn fill_aes_1rx4_soft(state: &mut [u8; 64], output: &mut [u8]) {
    let mut s0 = load_block(state, 0);
    let mut s1 = load_block(state, 16);
    let mut s2 = load_block(state, 32);
    let mut s3 = load_block(state, 48);

    let mut offset = 0;
    while offset < output.len() {
        s0 = soft_aesdec(&s0, &GEN_1R_KEY0);
        s1 = soft_aesenc(&s1, &GEN_1R_KEY1);
        s2 = soft_aesdec(&s2, &GEN_1R_KEY2);
        s3 = soft_aesenc(&s3, &GEN_1R_KEY3);

        store_block(output, offset, &s0);
        store_block(output, offset + 16, &s1);
        store_block(output, offset + 32, &s2);
        store_block(output, offset + 48, &s3);
        offset += 64;
    }

    store_block(state, 0, &s0);
    store_block(state, 16, &s1);
    store_block(state, 32, &s2);
    store_block(state, 48, &s3);
}

fn fill_aes_4rx4_soft(state: &[u8; 64], output: &mut [u8]) {
    let mut s0 = load_block(state, 0);
    let mut s1 = load_block(state, 16);
    let mut s2 = load_block(state, 32);
    let mut s3 = load_block(state, 48);

    let mut offset = 0;
    while offset < output.len() {
        s0 = soft_aesdec(&s0, &GEN_4R_KEY0);
        s1 = soft_aesenc(&s1, &GEN_4R_KEY0);
        s2 = soft_aesdec(&s2, &GEN_4R_KEY4);
        s3 = soft_aesenc(&s3, &GEN_4R_KEY4);

        s0 = soft_aesdec(&s0, &GEN_4R_KEY1);
        s1 = soft_aesenc(&s1, &GEN_4R_KEY1);
        s2 = soft_aesdec(&s2, &GEN_4R_KEY5);
        s3 = soft_aesenc(&s3, &GEN_4R_KEY5);

        s0 = soft_aesdec(&s0, &GEN_4R_KEY2);
        s1 = soft_aesenc(&s1, &GEN_4R_KEY2);
        s2 = soft_aesdec(&s2, &GEN_4R_KEY6);
        s3 = soft_aesenc(&s3, &GEN_4R_KEY6);

        s0 = soft_aesdec(&s0, &GEN_4R_KEY3);
        s1 = soft_aesenc(&s1, &GEN_4R_KEY3);
        s2 = soft_aesdec(&s2, &GEN_4R_KEY7);
        s3 = soft_aesenc(&s3, &GEN_4R_KEY7);

        store_block(output, offset, &s0);
        store_block(output, offset + 16, &s1);
        store_block(output, offset + 32, &s2);
        store_block(output, offset + 48, &s3);
        offset += 64;
    }
}

fn hash_aes_1rx4_soft(input: &[u8]) -> [u8; 64] {
    let mut s0 = HASH_1R_STATE0;
    let mut s1 = HASH_1R_STATE1;
    let mut s2 = HASH_1R_STATE2;
    let mut s3 = HASH_1R_STATE3;

    let mut offset = 0;
    while offset < input.len() {
        let in0 = load_block(input, offset);
        let in1 = load_block(input, offset + 16);
        let in2 = load_block(input, offset + 32);
        let in3 = load_block(input, offset + 48);
        s0 = soft_aesenc(&s0, &in0);
        s1 = soft_aesdec(&s1, &in1);
        s2 = soft_aesenc(&s2, &in2);
        s3 = soft_aesdec(&s3, &in3);
        offset += 64;
    }

    s0 = soft_aesenc(&s0, &HASH_1R_XKEY0);
    s1 = soft_aesdec(&s1, &HASH_1R_XKEY0);
    s2 = soft_aesenc(&s2, &HASH_1R_XKEY0);
    s3 = soft_aesdec(&s3, &HASH_1R_XKEY0);

    s0 = soft_aesenc(&s0, &HASH_1R_XKEY1);
    s1 = soft_aesdec(&s1, &HASH_1R_XKEY1);
    s2 = soft_aesenc(&s2, &HASH_1R_XKEY1);
    s3 = soft_aesdec(&s3, &HASH_1R_XKEY1);

    let mut out = [0u8; 64];
    store_block(&mut out, 0, &s0);
    store_block(&mut out, 16, &s1);
    store_block(&mut out, 32, &s2);
    store_block(&mut out, 48, &s3);
    out
}

fn hash_and_fill_aes_1rx4_soft(scratchpad: &mut [u8], hash_out: &mut [u8; 64], fill_state: &mut [u8; 64]) {
    // Hash state
    let mut hs0 = HASH_1R_STATE0;
    let mut hs1 = HASH_1R_STATE1;
    let mut hs2 = HASH_1R_STATE2;
    let mut hs3 = HASH_1R_STATE3;

    // Fill state
    let mut fs0 = load_block(fill_state, 0);
    let mut fs1 = load_block(fill_state, 16);
    let mut fs2 = load_block(fill_state, 32);
    let mut fs3 = load_block(fill_state, 48);

    let mut offset = 0;
    while offset < scratchpad.len() {
        let in0 = load_block(scratchpad, offset);
        let in1 = load_block(scratchpad, offset + 16);
        let in2 = load_block(scratchpad, offset + 32);
        let in3 = load_block(scratchpad, offset + 48);

        hs0 = soft_aesenc(&hs0, &in0);
        hs1 = soft_aesdec(&hs1, &in1);
        hs2 = soft_aesenc(&hs2, &in2);
        hs3 = soft_aesdec(&hs3, &in3);

        fs0 = soft_aesdec(&fs0, &GEN_1R_KEY0);
        fs1 = soft_aesenc(&fs1, &GEN_1R_KEY1);
        fs2 = soft_aesdec(&fs2, &GEN_1R_KEY2);
        fs3 = soft_aesenc(&fs3, &GEN_1R_KEY3);

        store_block(scratchpad, offset, &fs0);
        store_block(scratchpad, offset + 16, &fs1);
        store_block(scratchpad, offset + 32, &fs2);
        store_block(scratchpad, offset + 48, &fs3);
        offset += 64;
    }

    store_block(fill_state, 0, &fs0);
    store_block(fill_state, 16, &fs1);
    store_block(fill_state, 32, &fs2);
    store_block(fill_state, 48, &fs3);

    hs0 = soft_aesenc(&hs0, &HASH_1R_XKEY0);
    hs1 = soft_aesdec(&hs1, &HASH_1R_XKEY0);
    hs2 = soft_aesenc(&hs2, &HASH_1R_XKEY0);
    hs3 = soft_aesdec(&hs3, &HASH_1R_XKEY0);

    hs0 = soft_aesenc(&hs0, &HASH_1R_XKEY1);
    hs1 = soft_aesdec(&hs1, &HASH_1R_XKEY1);
    hs2 = soft_aesenc(&hs2, &HASH_1R_XKEY1);
    hs3 = soft_aesdec(&hs3, &HASH_1R_XKEY1);

    store_block(hash_out, 0, &hs0);
    store_block(hash_out, 16, &hs1);
    store_block(hash_out, 32, &hs2);
    store_block(hash_out, 48, &hs3);
}

// ============================================================================
// aarch64 NEON AES intrinsics — all state kept in SIMD registers
// ============================================================================
// ARM AES equivalences:
//   aesenc(s, k) = AESMC(AESE(s, zero)) ^ k
//   aesdec(s, k) = AESIMC(AESD(s, zero)) ^ k

#[cfg(target_arch = "aarch64")]
#[target_feature(enable = "aes,neon")]
unsafe fn fill_aes_1rx4_neon(state: &mut [u8; 64], output: &mut [u8]) { unsafe {
    use std::arch::aarch64::*;
    let zero = vmovq_n_u8(0);

    let mut s0 = vld1q_u8(state.as_ptr());
    let mut s1 = vld1q_u8(state.as_ptr().add(16));
    let mut s2 = vld1q_u8(state.as_ptr().add(32));
    let mut s3 = vld1q_u8(state.as_ptr().add(48));

    let k0 = vld1q_u8(GEN_1R_KEY0.as_ptr());
    let k1 = vld1q_u8(GEN_1R_KEY1.as_ptr());
    let k2 = vld1q_u8(GEN_1R_KEY2.as_ptr());
    let k3 = vld1q_u8(GEN_1R_KEY3.as_ptr());

    let out = output.as_mut_ptr();
    let mut offset = 0usize;
    while offset < output.len() {
        s0 = veorq_u8(vaesimcq_u8(vaesdq_u8(s0, zero)), k0);  // aesdec
        s1 = veorq_u8(vaesmcq_u8(vaeseq_u8(s1, zero)), k1);   // aesenc
        s2 = veorq_u8(vaesimcq_u8(vaesdq_u8(s2, zero)), k2);  // aesdec
        s3 = veorq_u8(vaesmcq_u8(vaeseq_u8(s3, zero)), k3);   // aesenc

        vst1q_u8(out.add(offset), s0);
        vst1q_u8(out.add(offset + 16), s1);
        vst1q_u8(out.add(offset + 32), s2);
        vst1q_u8(out.add(offset + 48), s3);
        offset += 64;
    }

    vst1q_u8(state.as_mut_ptr(), s0);
    vst1q_u8(state.as_mut_ptr().add(16), s1);
    vst1q_u8(state.as_mut_ptr().add(32), s2);
    vst1q_u8(state.as_mut_ptr().add(48), s3);
}}

#[cfg(target_arch = "aarch64")]
#[target_feature(enable = "aes,neon")]
unsafe fn fill_aes_4rx4_neon(state: &[u8; 64], output: &mut [u8]) { unsafe {
    use std::arch::aarch64::*;
    let zero = vmovq_n_u8(0);

    let mut s0 = vld1q_u8(state.as_ptr());
    let mut s1 = vld1q_u8(state.as_ptr().add(16));
    let mut s2 = vld1q_u8(state.as_ptr().add(32));
    let mut s3 = vld1q_u8(state.as_ptr().add(48));

    let rk0 = vld1q_u8(GEN_4R_KEY0.as_ptr());
    let rk1 = vld1q_u8(GEN_4R_KEY1.as_ptr());
    let rk2 = vld1q_u8(GEN_4R_KEY2.as_ptr());
    let rk3 = vld1q_u8(GEN_4R_KEY3.as_ptr());
    let rk4 = vld1q_u8(GEN_4R_KEY4.as_ptr());
    let rk5 = vld1q_u8(GEN_4R_KEY5.as_ptr());
    let rk6 = vld1q_u8(GEN_4R_KEY6.as_ptr());
    let rk7 = vld1q_u8(GEN_4R_KEY7.as_ptr());

    let out = output.as_mut_ptr();
    let mut offset = 0usize;
    while offset < output.len() {
        s0 = veorq_u8(vaesimcq_u8(vaesdq_u8(s0, zero)), rk0);
        s1 = veorq_u8(vaesmcq_u8(vaeseq_u8(s1, zero)), rk0);
        s2 = veorq_u8(vaesimcq_u8(vaesdq_u8(s2, zero)), rk4);
        s3 = veorq_u8(vaesmcq_u8(vaeseq_u8(s3, zero)), rk4);

        s0 = veorq_u8(vaesimcq_u8(vaesdq_u8(s0, zero)), rk1);
        s1 = veorq_u8(vaesmcq_u8(vaeseq_u8(s1, zero)), rk1);
        s2 = veorq_u8(vaesimcq_u8(vaesdq_u8(s2, zero)), rk5);
        s3 = veorq_u8(vaesmcq_u8(vaeseq_u8(s3, zero)), rk5);

        s0 = veorq_u8(vaesimcq_u8(vaesdq_u8(s0, zero)), rk2);
        s1 = veorq_u8(vaesmcq_u8(vaeseq_u8(s1, zero)), rk2);
        s2 = veorq_u8(vaesimcq_u8(vaesdq_u8(s2, zero)), rk6);
        s3 = veorq_u8(vaesmcq_u8(vaeseq_u8(s3, zero)), rk6);

        s0 = veorq_u8(vaesimcq_u8(vaesdq_u8(s0, zero)), rk3);
        s1 = veorq_u8(vaesmcq_u8(vaeseq_u8(s1, zero)), rk3);
        s2 = veorq_u8(vaesimcq_u8(vaesdq_u8(s2, zero)), rk7);
        s3 = veorq_u8(vaesmcq_u8(vaeseq_u8(s3, zero)), rk7);

        vst1q_u8(out.add(offset), s0);
        vst1q_u8(out.add(offset + 16), s1);
        vst1q_u8(out.add(offset + 32), s2);
        vst1q_u8(out.add(offset + 48), s3);
        offset += 64;
    }
}}

#[cfg(target_arch = "aarch64")]
#[target_feature(enable = "aes,neon")]
unsafe fn hash_aes_1rx4_neon(input: &[u8]) -> [u8; 64] { unsafe {
    use std::arch::aarch64::*;
    let zero = vmovq_n_u8(0);

    let mut s0 = vld1q_u8(HASH_1R_STATE0.as_ptr());
    let mut s1 = vld1q_u8(HASH_1R_STATE1.as_ptr());
    let mut s2 = vld1q_u8(HASH_1R_STATE2.as_ptr());
    let mut s3 = vld1q_u8(HASH_1R_STATE3.as_ptr());

    let inp = input.as_ptr();
    let mut offset = 0usize;
    while offset < input.len() {
        let in0 = vld1q_u8(inp.add(offset));
        let in1 = vld1q_u8(inp.add(offset + 16));
        let in2 = vld1q_u8(inp.add(offset + 32));
        let in3 = vld1q_u8(inp.add(offset + 48));
        // hash uses input blocks as keys
        s0 = veorq_u8(vaesmcq_u8(vaeseq_u8(s0, zero)), in0);   // aesenc
        s1 = veorq_u8(vaesimcq_u8(vaesdq_u8(s1, zero)), in1);  // aesdec
        s2 = veorq_u8(vaesmcq_u8(vaeseq_u8(s2, zero)), in2);
        s3 = veorq_u8(vaesimcq_u8(vaesdq_u8(s3, zero)), in3);
        offset += 64;
    }

    // Two finalization rounds
    let xk0 = vld1q_u8(HASH_1R_XKEY0.as_ptr());
    let xk1 = vld1q_u8(HASH_1R_XKEY1.as_ptr());

    s0 = veorq_u8(vaesmcq_u8(vaeseq_u8(s0, zero)), xk0);
    s1 = veorq_u8(vaesimcq_u8(vaesdq_u8(s1, zero)), xk0);
    s2 = veorq_u8(vaesmcq_u8(vaeseq_u8(s2, zero)), xk0);
    s3 = veorq_u8(vaesimcq_u8(vaesdq_u8(s3, zero)), xk0);

    s0 = veorq_u8(vaesmcq_u8(vaeseq_u8(s0, zero)), xk1);
    s1 = veorq_u8(vaesimcq_u8(vaesdq_u8(s1, zero)), xk1);
    s2 = veorq_u8(vaesmcq_u8(vaeseq_u8(s2, zero)), xk1);
    s3 = veorq_u8(vaesimcq_u8(vaesdq_u8(s3, zero)), xk1);

    let mut out = [0u8; 64];
    vst1q_u8(out.as_mut_ptr(), s0);
    vst1q_u8(out.as_mut_ptr().add(16), s1);
    vst1q_u8(out.as_mut_ptr().add(32), s2);
    vst1q_u8(out.as_mut_ptr().add(48), s3);
    out
}}

#[cfg(target_arch = "aarch64")]
#[target_feature(enable = "aes,neon")]
unsafe fn hash_and_fill_aes_1rx4_neon(scratchpad: &mut [u8], hash_out: &mut [u8; 64], fill_state: &mut [u8; 64]) { unsafe {
    use std::arch::aarch64::*;
    let zero = vmovq_n_u8(0);

    // Hash state
    let mut hs0 = vld1q_u8(HASH_1R_STATE0.as_ptr());
    let mut hs1 = vld1q_u8(HASH_1R_STATE1.as_ptr());
    let mut hs2 = vld1q_u8(HASH_1R_STATE2.as_ptr());
    let mut hs3 = vld1q_u8(HASH_1R_STATE3.as_ptr());

    // Fill state
    let mut fs0 = vld1q_u8(fill_state.as_ptr());
    let mut fs1 = vld1q_u8(fill_state.as_ptr().add(16));
    let mut fs2 = vld1q_u8(fill_state.as_ptr().add(32));
    let mut fs3 = vld1q_u8(fill_state.as_ptr().add(48));

    let fk0 = vld1q_u8(GEN_1R_KEY0.as_ptr());
    let fk1 = vld1q_u8(GEN_1R_KEY1.as_ptr());
    let fk2 = vld1q_u8(GEN_1R_KEY2.as_ptr());
    let fk3 = vld1q_u8(GEN_1R_KEY3.as_ptr());

    let sp = scratchpad.as_mut_ptr();
    let len = scratchpad.len();
    let mut offset = 0usize;
    while offset < len {
        // Hash: use current scratchpad data as round keys
        let in0 = vld1q_u8(sp.add(offset));
        let in1 = vld1q_u8(sp.add(offset + 16));
        let in2 = vld1q_u8(sp.add(offset + 32));
        let in3 = vld1q_u8(sp.add(offset + 48));

        hs0 = veorq_u8(vaesmcq_u8(vaeseq_u8(hs0, zero)), in0);  // aesenc
        hs1 = veorq_u8(vaesimcq_u8(vaesdq_u8(hs1, zero)), in1); // aesdec
        hs2 = veorq_u8(vaesmcq_u8(vaeseq_u8(hs2, zero)), in2);
        hs3 = veorq_u8(vaesimcq_u8(vaesdq_u8(hs3, zero)), in3);

        // Fill: generate new data
        fs0 = veorq_u8(vaesimcq_u8(vaesdq_u8(fs0, zero)), fk0);
        fs1 = veorq_u8(vaesmcq_u8(vaeseq_u8(fs1, zero)), fk1);
        fs2 = veorq_u8(vaesimcq_u8(vaesdq_u8(fs2, zero)), fk2);
        fs3 = veorq_u8(vaesmcq_u8(vaeseq_u8(fs3, zero)), fk3);

        // Write fill back to scratchpad
        vst1q_u8(sp.add(offset), fs0);
        vst1q_u8(sp.add(offset + 16), fs1);
        vst1q_u8(sp.add(offset + 32), fs2);
        vst1q_u8(sp.add(offset + 48), fs3);
        offset += 64;
    }

    // Save fill state
    vst1q_u8(fill_state.as_mut_ptr(), fs0);
    vst1q_u8(fill_state.as_mut_ptr().add(16), fs1);
    vst1q_u8(fill_state.as_mut_ptr().add(32), fs2);
    vst1q_u8(fill_state.as_mut_ptr().add(48), fs3);

    // Hash finalization
    let xk0 = vld1q_u8(HASH_1R_XKEY0.as_ptr());
    let xk1 = vld1q_u8(HASH_1R_XKEY1.as_ptr());

    hs0 = veorq_u8(vaesmcq_u8(vaeseq_u8(hs0, zero)), xk0);
    hs1 = veorq_u8(vaesimcq_u8(vaesdq_u8(hs1, zero)), xk0);
    hs2 = veorq_u8(vaesmcq_u8(vaeseq_u8(hs2, zero)), xk0);
    hs3 = veorq_u8(vaesimcq_u8(vaesdq_u8(hs3, zero)), xk0);

    hs0 = veorq_u8(vaesmcq_u8(vaeseq_u8(hs0, zero)), xk1);
    hs1 = veorq_u8(vaesimcq_u8(vaesdq_u8(hs1, zero)), xk1);
    hs2 = veorq_u8(vaesmcq_u8(vaeseq_u8(hs2, zero)), xk1);
    hs3 = veorq_u8(vaesimcq_u8(vaesdq_u8(hs3, zero)), xk1);

    vst1q_u8(hash_out.as_mut_ptr(), hs0);
    vst1q_u8(hash_out.as_mut_ptr().add(16), hs1);
    vst1q_u8(hash_out.as_mut_ptr().add(32), hs2);
    vst1q_u8(hash_out.as_mut_ptr().add(48), hs3);
}}

// ============================================================================
// x86_64 AES-NI intrinsics — all state kept in __m128i registers
// ============================================================================

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "aes,sse2")]
unsafe fn fill_aes_1rx4_aesni(state: &mut [u8; 64], output: &mut [u8]) {
    use std::arch::x86_64::*;

    let mut s0 = _mm_loadu_si128(state.as_ptr() as *const __m128i);
    let mut s1 = _mm_loadu_si128(state.as_ptr().add(16) as *const __m128i);
    let mut s2 = _mm_loadu_si128(state.as_ptr().add(32) as *const __m128i);
    let mut s3 = _mm_loadu_si128(state.as_ptr().add(48) as *const __m128i);

    let k0 = _mm_loadu_si128(GEN_1R_KEY0.as_ptr() as *const __m128i);
    let k1 = _mm_loadu_si128(GEN_1R_KEY1.as_ptr() as *const __m128i);
    let k2 = _mm_loadu_si128(GEN_1R_KEY2.as_ptr() as *const __m128i);
    let k3 = _mm_loadu_si128(GEN_1R_KEY3.as_ptr() as *const __m128i);

    let out = output.as_mut_ptr() as *mut __m128i;
    let len = output.len();
    let mut i = 0usize;
    while i * 64 < len {
        s0 = _mm_aesdec_si128(s0, k0);
        s1 = _mm_aesenc_si128(s1, k1);
        s2 = _mm_aesdec_si128(s2, k2);
        s3 = _mm_aesenc_si128(s3, k3);

        _mm_storeu_si128(out.add(i * 4), s0);
        _mm_storeu_si128(out.add(i * 4 + 1), s1);
        _mm_storeu_si128(out.add(i * 4 + 2), s2);
        _mm_storeu_si128(out.add(i * 4 + 3), s3);
        i += 1;
    }

    _mm_storeu_si128(state.as_mut_ptr() as *mut __m128i, s0);
    _mm_storeu_si128(state.as_mut_ptr().add(16) as *mut __m128i, s1);
    _mm_storeu_si128(state.as_mut_ptr().add(32) as *mut __m128i, s2);
    _mm_storeu_si128(state.as_mut_ptr().add(48) as *mut __m128i, s3);
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "aes,sse2")]
unsafe fn fill_aes_4rx4_aesni(state: &[u8; 64], output: &mut [u8]) {
    use std::arch::x86_64::*;

    let mut s0 = _mm_loadu_si128(state.as_ptr() as *const __m128i);
    let mut s1 = _mm_loadu_si128(state.as_ptr().add(16) as *const __m128i);
    let mut s2 = _mm_loadu_si128(state.as_ptr().add(32) as *const __m128i);
    let mut s3 = _mm_loadu_si128(state.as_ptr().add(48) as *const __m128i);

    let rk0 = _mm_loadu_si128(GEN_4R_KEY0.as_ptr() as *const __m128i);
    let rk1 = _mm_loadu_si128(GEN_4R_KEY1.as_ptr() as *const __m128i);
    let rk2 = _mm_loadu_si128(GEN_4R_KEY2.as_ptr() as *const __m128i);
    let rk3 = _mm_loadu_si128(GEN_4R_KEY3.as_ptr() as *const __m128i);
    let rk4 = _mm_loadu_si128(GEN_4R_KEY4.as_ptr() as *const __m128i);
    let rk5 = _mm_loadu_si128(GEN_4R_KEY5.as_ptr() as *const __m128i);
    let rk6 = _mm_loadu_si128(GEN_4R_KEY6.as_ptr() as *const __m128i);
    let rk7 = _mm_loadu_si128(GEN_4R_KEY7.as_ptr() as *const __m128i);

    let out = output.as_mut_ptr() as *mut __m128i;
    let len = output.len();
    let mut i = 0usize;
    while i * 64 < len {
        s0 = _mm_aesdec_si128(s0, rk0);
        s1 = _mm_aesenc_si128(s1, rk0);
        s2 = _mm_aesdec_si128(s2, rk4);
        s3 = _mm_aesenc_si128(s3, rk4);

        s0 = _mm_aesdec_si128(s0, rk1);
        s1 = _mm_aesenc_si128(s1, rk1);
        s2 = _mm_aesdec_si128(s2, rk5);
        s3 = _mm_aesenc_si128(s3, rk5);

        s0 = _mm_aesdec_si128(s0, rk2);
        s1 = _mm_aesenc_si128(s1, rk2);
        s2 = _mm_aesdec_si128(s2, rk6);
        s3 = _mm_aesenc_si128(s3, rk6);

        s0 = _mm_aesdec_si128(s0, rk3);
        s1 = _mm_aesenc_si128(s1, rk3);
        s2 = _mm_aesdec_si128(s2, rk7);
        s3 = _mm_aesenc_si128(s3, rk7);

        _mm_storeu_si128(out.add(i * 4), s0);
        _mm_storeu_si128(out.add(i * 4 + 1), s1);
        _mm_storeu_si128(out.add(i * 4 + 2), s2);
        _mm_storeu_si128(out.add(i * 4 + 3), s3);
        i += 1;
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "aes,sse2")]
unsafe fn hash_aes_1rx4_aesni(input: &[u8]) -> [u8; 64] {
    use std::arch::x86_64::*;

    let mut s0 = _mm_loadu_si128(HASH_1R_STATE0.as_ptr() as *const __m128i);
    let mut s1 = _mm_loadu_si128(HASH_1R_STATE1.as_ptr() as *const __m128i);
    let mut s2 = _mm_loadu_si128(HASH_1R_STATE2.as_ptr() as *const __m128i);
    let mut s3 = _mm_loadu_si128(HASH_1R_STATE3.as_ptr() as *const __m128i);

    let inp = input.as_ptr() as *const __m128i;
    let len = input.len();
    let mut i = 0usize;
    while i * 64 < len {
        let in0 = _mm_loadu_si128(inp.add(i * 4));
        let in1 = _mm_loadu_si128(inp.add(i * 4 + 1));
        let in2 = _mm_loadu_si128(inp.add(i * 4 + 2));
        let in3 = _mm_loadu_si128(inp.add(i * 4 + 3));
        s0 = _mm_aesenc_si128(s0, in0);
        s1 = _mm_aesdec_si128(s1, in1);
        s2 = _mm_aesenc_si128(s2, in2);
        s3 = _mm_aesdec_si128(s3, in3);
        i += 1;
    }

    let xk0 = _mm_loadu_si128(HASH_1R_XKEY0.as_ptr() as *const __m128i);
    let xk1 = _mm_loadu_si128(HASH_1R_XKEY1.as_ptr() as *const __m128i);

    s0 = _mm_aesenc_si128(s0, xk0);
    s1 = _mm_aesdec_si128(s1, xk0);
    s2 = _mm_aesenc_si128(s2, xk0);
    s3 = _mm_aesdec_si128(s3, xk0);

    s0 = _mm_aesenc_si128(s0, xk1);
    s1 = _mm_aesdec_si128(s1, xk1);
    s2 = _mm_aesenc_si128(s2, xk1);
    s3 = _mm_aesdec_si128(s3, xk1);

    let mut out = [0u8; 64];
    _mm_storeu_si128(out.as_mut_ptr() as *mut __m128i, s0);
    _mm_storeu_si128(out.as_mut_ptr().add(16) as *mut __m128i, s1);
    _mm_storeu_si128(out.as_mut_ptr().add(32) as *mut __m128i, s2);
    _mm_storeu_si128(out.as_mut_ptr().add(48) as *mut __m128i, s3);
    out
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "aes,sse2")]
unsafe fn hash_and_fill_aes_1rx4_aesni(scratchpad: &mut [u8], hash_out: &mut [u8; 64], fill_state: &mut [u8; 64]) {
    use std::arch::x86_64::*;

    // Hash state
    let mut hs0 = _mm_loadu_si128(HASH_1R_STATE0.as_ptr() as *const __m128i);
    let mut hs1 = _mm_loadu_si128(HASH_1R_STATE1.as_ptr() as *const __m128i);
    let mut hs2 = _mm_loadu_si128(HASH_1R_STATE2.as_ptr() as *const __m128i);
    let mut hs3 = _mm_loadu_si128(HASH_1R_STATE3.as_ptr() as *const __m128i);

    // Fill state
    let mut fs0 = _mm_loadu_si128(fill_state.as_ptr() as *const __m128i);
    let mut fs1 = _mm_loadu_si128(fill_state.as_ptr().add(16) as *const __m128i);
    let mut fs2 = _mm_loadu_si128(fill_state.as_ptr().add(32) as *const __m128i);
    let mut fs3 = _mm_loadu_si128(fill_state.as_ptr().add(48) as *const __m128i);

    let fk0 = _mm_loadu_si128(GEN_1R_KEY0.as_ptr() as *const __m128i);
    let fk1 = _mm_loadu_si128(GEN_1R_KEY1.as_ptr() as *const __m128i);
    let fk2 = _mm_loadu_si128(GEN_1R_KEY2.as_ptr() as *const __m128i);
    let fk3 = _mm_loadu_si128(GEN_1R_KEY3.as_ptr() as *const __m128i);

    let sp = scratchpad.as_mut_ptr() as *mut __m128i;
    let len = scratchpad.len();
    let mut i = 0usize;
    while i * 64 < len {
        // Hash: use current scratchpad data as round keys
        let in0 = _mm_loadu_si128(sp.add(i * 4));
        let in1 = _mm_loadu_si128(sp.add(i * 4 + 1));
        let in2 = _mm_loadu_si128(sp.add(i * 4 + 2));
        let in3 = _mm_loadu_si128(sp.add(i * 4 + 3));

        hs0 = _mm_aesenc_si128(hs0, in0);
        hs1 = _mm_aesdec_si128(hs1, in1);
        hs2 = _mm_aesenc_si128(hs2, in2);
        hs3 = _mm_aesdec_si128(hs3, in3);

        // Fill: generate new data
        fs0 = _mm_aesdec_si128(fs0, fk0);
        fs1 = _mm_aesenc_si128(fs1, fk1);
        fs2 = _mm_aesdec_si128(fs2, fk2);
        fs3 = _mm_aesenc_si128(fs3, fk3);

        // Write fill back to scratchpad
        _mm_storeu_si128(sp.add(i * 4), fs0);
        _mm_storeu_si128(sp.add(i * 4 + 1), fs1);
        _mm_storeu_si128(sp.add(i * 4 + 2), fs2);
        _mm_storeu_si128(sp.add(i * 4 + 3), fs3);
        i += 1;
    }

    // Save fill state
    _mm_storeu_si128(fill_state.as_mut_ptr() as *mut __m128i, fs0);
    _mm_storeu_si128(fill_state.as_mut_ptr().add(16) as *mut __m128i, fs1);
    _mm_storeu_si128(fill_state.as_mut_ptr().add(32) as *mut __m128i, fs2);
    _mm_storeu_si128(fill_state.as_mut_ptr().add(48) as *mut __m128i, fs3);

    // Hash finalization
    let xk0 = _mm_loadu_si128(HASH_1R_XKEY0.as_ptr() as *const __m128i);
    let xk1 = _mm_loadu_si128(HASH_1R_XKEY1.as_ptr() as *const __m128i);

    hs0 = _mm_aesenc_si128(hs0, xk0);
    hs1 = _mm_aesdec_si128(hs1, xk0);
    hs2 = _mm_aesenc_si128(hs2, xk0);
    hs3 = _mm_aesdec_si128(hs3, xk0);

    hs0 = _mm_aesenc_si128(hs0, xk1);
    hs1 = _mm_aesdec_si128(hs1, xk1);
    hs2 = _mm_aesenc_si128(hs2, xk1);
    hs3 = _mm_aesdec_si128(hs3, xk1);

    _mm_storeu_si128(hash_out.as_mut_ptr() as *mut __m128i, hs0);
    _mm_storeu_si128(hash_out.as_mut_ptr().add(16) as *mut __m128i, hs1);
    _mm_storeu_si128(hash_out.as_mut_ptr().add(32) as *mut __m128i, hs2);
    _mm_storeu_si128(hash_out.as_mut_ptr().add(48) as *mut __m128i, hs3);
}
