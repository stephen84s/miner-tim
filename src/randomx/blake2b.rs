// Blake2b hash functions (RFC 7693)
// Pure Rust implementation — no external crate dependencies.

/// Blake2b initialization vector.
const IV: [u64; 8] = [
    0x6a09e667f3bcc908,
    0xbb67ae8584caa73b,
    0x3c6ef372fe94f82b,
    0xa54ff53a5f1d36f1,
    0x510e527fade682d1,
    0x9b05688c2b3e6c1f,
    0x1f83d9abfb41bd6b,
    0x5be0cd19137e2179,
];

/// Sigma permutation table (12 rounds × 16 entries).
const SIGMA: [[usize; 16]; 12] = [
    [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15],
    [14, 10, 4, 8, 9, 15, 13, 6, 1, 12, 0, 2, 11, 7, 5, 3],
    [11, 8, 12, 0, 5, 2, 15, 13, 10, 14, 3, 6, 7, 1, 9, 4],
    [7, 9, 3, 1, 13, 12, 11, 14, 2, 6, 5, 10, 4, 0, 15, 8],
    [9, 0, 5, 7, 2, 4, 10, 15, 14, 1, 11, 12, 6, 8, 3, 13],
    [2, 12, 6, 10, 0, 11, 8, 3, 4, 13, 7, 5, 15, 14, 1, 9],
    [12, 5, 1, 15, 14, 13, 4, 10, 0, 7, 6, 3, 9, 2, 8, 11],
    [13, 11, 7, 14, 12, 1, 3, 9, 5, 0, 15, 4, 8, 6, 2, 10],
    [6, 15, 14, 9, 11, 3, 0, 8, 12, 2, 13, 7, 1, 4, 10, 5],
    [10, 2, 8, 4, 7, 6, 1, 5, 15, 11, 9, 14, 3, 12, 13, 0],
    [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15],
    [14, 10, 4, 8, 9, 15, 13, 6, 1, 12, 0, 2, 11, 7, 5, 3],
];

/// G mixing function.
#[inline(always)]
fn g(v: &mut [u64; 16], a: usize, b: usize, c: usize, d: usize, x: u64, y: u64) {
    v[a] = v[a].wrapping_add(v[b]).wrapping_add(x);
    v[d] = (v[d] ^ v[a]).rotate_right(32);
    v[c] = v[c].wrapping_add(v[d]);
    v[b] = (v[b] ^ v[c]).rotate_right(24);
    v[a] = v[a].wrapping_add(v[b]).wrapping_add(y);
    v[d] = (v[d] ^ v[a]).rotate_right(16);
    v[c] = v[c].wrapping_add(v[d]);
    v[b] = (v[b] ^ v[c]).rotate_right(63);
}

/// Compress one block.
fn compress(h: &mut [u64; 8], block: &[u8; 128], t: u128, last: bool) {
    let mut v = [0u64; 16];
    v[..8].copy_from_slice(h);
    v[8..16].copy_from_slice(&IV);

    v[12] ^= t as u64;
    v[13] ^= (t >> 64) as u64;
    if last {
        v[14] = !v[14];
    }

    // Parse message block into 16 u64 words (little-endian).
    let mut m = [0u64; 16];
    for i in 0..16 {
        m[i] = u64::from_le_bytes(block[i * 8..i * 8 + 8].try_into().unwrap());
    }

    // 12 rounds of mixing.
    for i in 0..12 {
        let s = &SIGMA[i];
        g(&mut v, 0, 4, 8, 12, m[s[0]], m[s[1]]);
        g(&mut v, 1, 5, 9, 13, m[s[2]], m[s[3]]);
        g(&mut v, 2, 6, 10, 14, m[s[4]], m[s[5]]);
        g(&mut v, 3, 7, 11, 15, m[s[6]], m[s[7]]);
        g(&mut v, 0, 5, 10, 15, m[s[8]], m[s[9]]);
        g(&mut v, 1, 6, 11, 12, m[s[10]], m[s[11]]);
        g(&mut v, 2, 7, 8, 13, m[s[12]], m[s[13]]);
        g(&mut v, 3, 4, 9, 14, m[s[14]], m[s[15]]);
    }

    for i in 0..8 {
        h[i] ^= v[i] ^ v[i + 8];
    }
}

/// Compute Blake2b hash with variable output length (1..=64) and optional key (0..=64).
pub fn blake2b_full(out_len: usize, key: &[u8], input: &[u8]) -> Vec<u8> {
    assert!((1..=64).contains(&out_len));
    assert!(key.len() <= 64);

    let kk = key.len();

    // Initialize state with parameter block xored into IV.
    // Parameter block (simplified): p[0] = 0x01010000 ^ (kk << 8) ^ nn
    let mut h = IV;
    h[0] ^= 0x01010000 ^ ((kk as u64) << 8) ^ (out_len as u64);

    let mut t: u128 = 0;
    let mut buf = [0u8; 128];
    let mut buf_len: usize = 0;

    // If keyed, the first block is the key padded to 128 bytes.
    if kk > 0 {
        buf[..kk].copy_from_slice(key);
        buf_len = 128;
    }

    // Process input.
    for &byte in input {
        if buf_len == 128 {
            t += 128;
            compress(&mut h, &buf, t, false);
            buf = [0u8; 128];
            buf_len = 0;
        }
        buf[buf_len] = byte;
        buf_len += 1;
    }

    // Final block.
    // If we have a key but no input, the key block is the final (and only) block.
    t += buf_len as u128;
    // Pad remaining with zeros (buf is already zero-initialized above where needed).
    // Actually buf was zeroed on creation, and we only wrote buf_len bytes, so rest is 0.
    // But we need to ensure the full 128 bytes are zero-padded.
    for i in buf_len..128 {
        buf[i] = 0;
    }
    compress(&mut h, &buf, t, true);

    // Produce output.
    let mut out = Vec::with_capacity(out_len);
    for i in 0..out_len {
        out.push((h[i / 8] >> (8 * (i % 8))) as u8);
    }
    out
}

/// Compute Blake2b hash with variable output length, no key.
pub fn blake2b(out_len: usize, input: &[u8]) -> Vec<u8> {
    blake2b_full(out_len, &[], input)
}

/// Blake2b with 32-byte output.
pub fn blake2b_256(input: &[u8]) -> [u8; 32] {
    let v = blake2b(32, input);
    v.try_into().unwrap()
}

/// Blake2b with 64-byte output.
pub fn blake2b_512(input: &[u8]) -> [u8; 64] {
    let v = blake2b(64, input);
    v.try_into().unwrap()
}
