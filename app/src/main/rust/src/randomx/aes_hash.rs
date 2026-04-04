// AES-based generator and hash functions
// Reference: RandomX src/aes_hash.cpp
//
// Key convention: rx_set_int_vec_i128(a, b, c, d) stores as little-endian
// bytes: [a_le32 || b_le32 || c_le32 || d_le32] in memory.

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

// AesGenerator1R keys = Blake2b-512("RandomX AesGenerator1R keys")
pub(super) const GEN_1R_KEY0: [u8; 16] = key_from_u32s(0xb4f44917, 0xdbb5552b, 0x62716609, 0x6daca553);
pub(super) const GEN_1R_KEY1: [u8; 16] = key_from_u32s(0x0da1dc4e, 0x1725d378, 0x846a710d, 0x6d7caf07);
pub(super) const GEN_1R_KEY2: [u8; 16] = key_from_u32s(0x3e20e345, 0xf4c0794f, 0x9f947ec6, 0x3f1262f1);
pub(super) const GEN_1R_KEY3: [u8; 16] = key_from_u32s(0x49169154, 0x16314c88, 0xb1ba317c, 0x6aef8135);

// AesGenerator4R keys = Blake2b-512("RandomX AesGenerator4R keys 0-3") + Blake2b-512("... keys 4-7")
const GEN_4R_KEY0: [u8; 16] = key_from_u32s(0x99e5d23f, 0x2f546d2b, 0xd1833ddb, 0x6421aadd);
const GEN_4R_KEY1: [u8; 16] = key_from_u32s(0xa5dfcde5, 0x06f79d53, 0xb6913f55, 0xb20e3450);
const GEN_4R_KEY2: [u8; 16] = key_from_u32s(0x171c02bf, 0x0aa4679f, 0x515e7baf, 0x5c3ed904);
const GEN_4R_KEY3: [u8; 16] = key_from_u32s(0xd8ded291, 0xcd673785, 0xe78f5d08, 0x85623763);
const GEN_4R_KEY4: [u8; 16] = key_from_u32s(0x229effb4, 0x3d518b6d, 0xe3d6a7a6, 0xb5826f73);
const GEN_4R_KEY5: [u8; 16] = key_from_u32s(0xb272b7d2, 0xe9024d4e, 0x9c10b3d9, 0xc7566bf3);
const GEN_4R_KEY6: [u8; 16] = key_from_u32s(0xf63befa7, 0x2ba9660a, 0xf765a38b, 0xf273c9e7);
const GEN_4R_KEY7: [u8; 16] = key_from_u32s(0xc0b0762d, 0x0c06d1fd, 0x915839de, 0x7a7cd609);

// AesHash1R initial state = Blake2b-512("RandomX AesHash1R state")
pub(super) const HASH_1R_STATE0: [u8; 16] = key_from_u32s(0xd7983aad, 0xcc82db47, 0x9fa856de, 0x92b52c0d);
pub(super) const HASH_1R_STATE1: [u8; 16] = key_from_u32s(0xace78057, 0xf59e125a, 0x15c7b798, 0x338d996e);
pub(super) const HASH_1R_STATE2: [u8; 16] = key_from_u32s(0xe8a07ce4, 0x5079506b, 0xae62c7d0, 0x6a770017);
pub(super) const HASH_1R_STATE3: [u8; 16] = key_from_u32s(0x7e994948, 0x79a10005, 0x07ad828d, 0x630a240c);

// AesHash1R extra keys = Blake2b-256("RandomX AesHash1R xkeys")
pub(super) const HASH_1R_XKEY0: [u8; 16] = key_from_u32s(0x06890201, 0x90dc56bf, 0x8b24949f, 0xf6fa8389);
pub(super) const HASH_1R_XKEY1: [u8; 16] = key_from_u32s(0xed18f99b, 0xee1043c6, 0x51f4e03c, 0x61b263d1);

fn load_block(data: &[u8], offset: usize) -> [u8; 16] {
    data[offset..offset + 16].try_into().unwrap()
}

fn store_block(data: &mut [u8], offset: usize, block: &[u8; 16]) {
    data[offset..offset + 16].copy_from_slice(block);
}

/// fillAes1Rx4: Fill output with pseudorandom data based on 64-byte state.
/// Uses 1 AES round per 16 bytes in 4 parallel lanes.
/// State is modified in-place (for chaining calls).
/// Output length must be a multiple of 64.
pub fn fill_aes_1rx4(state: &mut [u8; 64], output: &mut [u8]) {
    assert!(output.len() % 64 == 0);

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

    // Write modified state back
    store_block(state, 0, &s0);
    store_block(state, 16, &s1);
    store_block(state, 32, &s2);
    store_block(state, 48, &s3);
}

/// fillAes4Rx4: Fill output using 4 AES rounds per 64 bytes.
/// Used for program generation. Output length must be a multiple of 64.
/// Note: key pairing from C++ source:
///   state0 & state1 use keys 0-3
///   state2 & state3 use keys 4-7
pub fn fill_aes_4rx4(state: &[u8; 64], output: &mut [u8]) {
    assert!(output.len() % 64 == 0);

    let mut s0 = load_block(state, 0);
    let mut s1 = load_block(state, 16);
    let mut s2 = load_block(state, 32);
    let mut s3 = load_block(state, 48);

    let mut offset = 0;
    while offset < output.len() {
        // Round 1
        s0 = soft_aesdec(&s0, &GEN_4R_KEY0);
        s1 = soft_aesenc(&s1, &GEN_4R_KEY0);
        s2 = soft_aesdec(&s2, &GEN_4R_KEY4);
        s3 = soft_aesenc(&s3, &GEN_4R_KEY4);

        // Round 2
        s0 = soft_aesdec(&s0, &GEN_4R_KEY1);
        s1 = soft_aesenc(&s1, &GEN_4R_KEY1);
        s2 = soft_aesdec(&s2, &GEN_4R_KEY5);
        s3 = soft_aesenc(&s3, &GEN_4R_KEY5);

        // Round 3
        s0 = soft_aesdec(&s0, &GEN_4R_KEY2);
        s1 = soft_aesenc(&s1, &GEN_4R_KEY2);
        s2 = soft_aesdec(&s2, &GEN_4R_KEY6);
        s3 = soft_aesenc(&s3, &GEN_4R_KEY6);

        // Round 4
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

/// hashAes1Rx4: Hash input (multiple of 64 bytes) into 64-byte output.
/// The input blocks serve as AES round keys applied to the running state.
/// Pattern: enc, dec, enc, dec for the 4 lanes.
pub fn hash_aes_1rx4(input: &[u8]) -> [u8; 64] {
    assert!(input.len() % 64 == 0);

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

    // Two finalization rounds with extra keys
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
