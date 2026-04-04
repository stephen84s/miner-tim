// Argon2d cache initialization for RandomX
// Reference: RFC 9106 + RandomX dataset.cpp + argon2_core.c + argon2_ref.c
//
// RandomX parameters:
//   type = Argon2d (0)
//   version = 0x13
//   t_cost (passes) = 3
//   m_cost (memory KiB) = 262144
//   lanes = 1
//   salt = "RandomX\x03"
//   secret = none, ad = none, outlen = 0
//   password = key

use super::blake2b;

const ARGON2_BLOCK_SIZE: usize = 1024;
const ARGON2_QWORDS_IN_BLOCK: usize = ARGON2_BLOCK_SIZE / 8;
const ARGON2_SYNC_POINTS: u32 = 4;
const ARGON2_PREHASH_DIGEST_LENGTH: usize = 64;
const ARGON2_PREHASH_SEED_LENGTH: usize = 72;
const ARGON2_VERSION: u32 = 0x13;

// RandomX-specific constants
const RANDOMX_ARGON_MEMORY: u32 = 262144; // KiB
const RANDOMX_ARGON_ITERATIONS: u32 = 3;
const RANDOMX_ARGON_LANES: u32 = 1;
const RANDOMX_ARGON_SALT: &[u8] = b"RandomX\x03";

/// A 1024-byte block as 128 u64 values.
#[derive(Clone)]
struct Block {
    v: [u64; ARGON2_QWORDS_IN_BLOCK],
}

impl Block {
    fn new() -> Self {
        Block {
            v: [0u64; ARGON2_QWORDS_IN_BLOCK],
        }
    }

    fn xor_with(&mut self, other: &Block) {
        for i in 0..ARGON2_QWORDS_IN_BLOCK {
            self.v[i] ^= other.v[i];
        }
    }

    /// Load from raw bytes (little-endian u64s).
    fn load_from_bytes(&mut self, bytes: &[u8]) {
        for i in 0..ARGON2_QWORDS_IN_BLOCK {
            self.v[i] = u64::from_le_bytes(bytes[i * 8..i * 8 + 8].try_into().unwrap());
        }
    }

    /// Store to raw bytes (little-endian u64s).
    fn store_to_bytes(&self, bytes: &mut [u8]) {
        for i in 0..ARGON2_QWORDS_IN_BLOCK {
            bytes[i * 8..i * 8 + 8].copy_from_slice(&self.v[i].to_le_bytes());
        }
    }
}

/// BlaMka mixing function: x + y + 2 * trunc(x) * trunc(y)
#[inline(always)]
fn f_bla_mka(x: u64, y: u64) -> u64 {
    let m: u64 = 0xFFFFFFFF;
    let xy = (x & m).wrapping_mul(y & m);
    x.wrapping_add(y).wrapping_add(2u64.wrapping_mul(xy))
}

/// G mixing function for Argon2 (BlaMka variant of Blake2).
/// Operates on array elements by index to satisfy borrow checker.
#[inline(always)]
fn g_blamka(v: &mut [u64; 16], a: usize, b: usize, c: usize, d: usize) {
    v[a] = f_bla_mka(v[a], v[b]);
    v[d] = (v[d] ^ v[a]).rotate_right(32);
    v[c] = f_bla_mka(v[c], v[d]);
    v[b] = (v[b] ^ v[c]).rotate_right(24);
    v[a] = f_bla_mka(v[a], v[b]);
    v[d] = (v[d] ^ v[a]).rotate_right(16);
    v[c] = f_bla_mka(v[c], v[d]);
    v[b] = (v[b] ^ v[c]).rotate_right(63);
}

/// BLAKE2_ROUND_NOMSG on 16 u64 values.
fn blake2_round_nomsg(v: &mut [u64; 16]) {
    // Column step
    g_blamka(v, 0, 4, 8, 12);
    g_blamka(v, 1, 5, 9, 13);
    g_blamka(v, 2, 6, 10, 14);
    g_blamka(v, 3, 7, 11, 15);
    // Diagonal step
    g_blamka(v, 0, 5, 10, 15);
    g_blamka(v, 1, 6, 11, 12);
    g_blamka(v, 2, 7, 8, 13);
    g_blamka(v, 3, 4, 9, 14);
}

/// fill_block: Argon2 compression function.
/// Computes next_block from prev_block and ref_block.
/// If with_xor, XORs result with existing next_block content.
fn fill_block(prev_block: &Block, ref_block: &Block, next_block: &mut Block, with_xor: bool) {
    let mut block_r = ref_block.clone();
    block_r.xor_with(prev_block);
    let mut block_tmp = block_r.clone();

    if with_xor {
        block_tmp.xor_with(next_block);
    }

    // Apply Blake2 on columns of 64-bit words: (0..15), (16..31), ..., (112..127)
    for i in 0..8 {
        let base = 16 * i;
        let mut v = [0u64; 16];
        for j in 0..16 {
            v[j] = block_r.v[base + j];
        }
        blake2_round_nomsg(&mut v);
        for j in 0..16 {
            block_r.v[base + j] = v[j];
        }
    }

    // Apply Blake2 on rows: (0,1,16,17,32,33,...,112,113), (2,3,18,19,...,114,115), ...
    for i in 0..8 {
        let mut v = [0u64; 16];
        v[0] = block_r.v[2 * i];
        v[1] = block_r.v[2 * i + 1];
        v[2] = block_r.v[2 * i + 16];
        v[3] = block_r.v[2 * i + 17];
        v[4] = block_r.v[2 * i + 32];
        v[5] = block_r.v[2 * i + 33];
        v[6] = block_r.v[2 * i + 48];
        v[7] = block_r.v[2 * i + 49];
        v[8] = block_r.v[2 * i + 64];
        v[9] = block_r.v[2 * i + 65];
        v[10] = block_r.v[2 * i + 80];
        v[11] = block_r.v[2 * i + 81];
        v[12] = block_r.v[2 * i + 96];
        v[13] = block_r.v[2 * i + 97];
        v[14] = block_r.v[2 * i + 112];
        v[15] = block_r.v[2 * i + 113];
        blake2_round_nomsg(&mut v);
        block_r.v[2 * i] = v[0];
        block_r.v[2 * i + 1] = v[1];
        block_r.v[2 * i + 16] = v[2];
        block_r.v[2 * i + 17] = v[3];
        block_r.v[2 * i + 32] = v[4];
        block_r.v[2 * i + 33] = v[5];
        block_r.v[2 * i + 48] = v[6];
        block_r.v[2 * i + 49] = v[7];
        block_r.v[2 * i + 64] = v[8];
        block_r.v[2 * i + 65] = v[9];
        block_r.v[2 * i + 80] = v[10];
        block_r.v[2 * i + 81] = v[11];
        block_r.v[2 * i + 96] = v[12];
        block_r.v[2 * i + 97] = v[13];
        block_r.v[2 * i + 112] = v[14];
        block_r.v[2 * i + 113] = v[15];
    }

    // next_block = block_tmp XOR block_r
    *next_block = block_tmp;
    next_block.xor_with(&block_r);
}

/// blake2b_long: Variable-length hash used by Argon2 for block generation.
/// Produces `out_len` bytes from input using iterated Blake2b.
fn blake2b_long(out_len: usize, input: &[u8]) -> Vec<u8> {
    let outlen_le = (out_len as u32).to_le_bytes();

    if out_len <= 64 {
        // Single Blake2b call with outlen prefix
        let mut data = Vec::with_capacity(4 + input.len());
        data.extend_from_slice(&outlen_le);
        data.extend_from_slice(input);
        return blake2b::blake2b(out_len, &data);
    }

    // Multi-block: first hash produces 64 bytes, take first 32
    let mut out = Vec::with_capacity(out_len);
    let mut data = Vec::with_capacity(4 + input.len());
    data.extend_from_slice(&outlen_le);
    data.extend_from_slice(input);

    let mut out_buffer = blake2b::blake2b_512(&data);
    out.extend_from_slice(&out_buffer[..32]);

    let mut toproduce = out_len - 32;

    while toproduce > 64 {
        let in_buffer = out_buffer;
        out_buffer = blake2b::blake2b_512(&in_buffer);
        out.extend_from_slice(&out_buffer[..32]);
        toproduce -= 32;
    }

    // Final block: hash with exact remaining length
    let final_hash = blake2b::blake2b(toproduce, &out_buffer);
    out.extend_from_slice(&final_hash);
    out
}

/// Compute the Argon2d index_alpha (reference block position).
fn index_alpha(
    pass: u32,
    slice: u32,
    index: u32,
    pseudo_rand: u32,
    same_lane: bool,
    lane_length: u32,
    segment_length: u32,
) -> u32 {
    let reference_area_size: u32;

    if pass == 0 {
        if slice == 0 {
            reference_area_size = index - 1;
        } else if same_lane {
            reference_area_size = slice * segment_length + index - 1;
        } else {
            reference_area_size =
                slice * segment_length + if index == 0 { u32::MAX } else { 0 };
            // C++ uses (-1) as uint32_t which wraps, but let's be more explicit:
            // When index==0 and !same_lane: reference_area_size = slice*segment_length - 1
        }
    } else if same_lane {
        reference_area_size = lane_length - segment_length + index - 1;
    } else {
        reference_area_size =
            lane_length - segment_length + if index == 0 { u32::MAX } else { 0 };
    }

    // Mapping pseudo_rand to 0..<reference_area_size-1>
    let relative_position = {
        let mut rp = pseudo_rand as u64;
        rp = rp.wrapping_mul(rp) >> 32;
        (reference_area_size as u64)
            .wrapping_sub((reference_area_size as u64).wrapping_mul(rp) >> 32)
            .wrapping_sub(1) as u32
    };

    // Starting position
    let start_position = if pass != 0 {
        if slice == ARGON2_SYNC_POINTS - 1 {
            0
        } else {
            (slice + 1) * segment_length
        }
    } else {
        0
    };

    (start_position + relative_position) % lane_length
}

/// Fill one segment of memory blocks.
fn fill_segment(
    memory: &mut [Block],
    pass: u32,
    lane: u32,
    slice: u8,
    lanes: u32,
    lane_length: u32,
    segment_length: u32,
) {
    let starting_index = if pass == 0 && slice == 0 { 2u32 } else { 0u32 };

    let mut curr_offset =
        lane * lane_length + (slice as u32) * segment_length + starting_index;

    let mut prev_offset = if curr_offset % lane_length == 0 {
        curr_offset + lane_length - 1
    } else {
        curr_offset - 1
    };

    for i in starting_index..segment_length {
        if curr_offset % lane_length == 1 {
            prev_offset = curr_offset - 1;
        }

        let pseudo_rand = memory[prev_offset as usize].v[0];
        let ref_lane = ((pseudo_rand >> 32) as u32) % lanes;

        let actual_ref_lane = if pass == 0 && slice == 0 {
            lane // Can not reference other lanes yet
        } else {
            ref_lane
        };

        let ref_index = index_alpha(
            pass,
            slice as u32,
            i,
            (pseudo_rand & 0xFFFFFFFF) as u32,
            actual_ref_lane == lane,
            lane_length,
            segment_length,
        );

        let ref_block_offset = (actual_ref_lane * lane_length + ref_index) as usize;
        let curr_block_offset = curr_offset as usize;
        let prev_block_offset = prev_offset as usize;

        // We need to borrow prev, ref, and curr from the same slice.
        // Use indices to avoid multiple mutable borrows.
        let with_xor = pass > 0; // version 0x13: XOR on pass > 0

        // Clone prev and ref to avoid borrow issues
        let prev = memory[prev_block_offset].clone();
        let ref_b = memory[ref_block_offset].clone();
        fill_block(&prev, &ref_b, &mut memory[curr_block_offset], with_xor);

        curr_offset += 1;
        prev_offset += 1;
    }
}

/// Compute the initial hash H0 for Argon2d.
/// H0 = Blake2b-64(lanes || outlen || m_cost || t_cost || version || type || pwdlen || pwd || saltlen || salt || secretlen || adlen)
fn initial_hash(key: &[u8]) -> [u8; ARGON2_PREHASH_DIGEST_LENGTH] {
    // Build the input to Blake2b incrementally.
    // We use our non-streaming Blake2b, so we build the full input buffer.
    let mut input = Vec::new();

    // lanes (u32 LE)
    input.extend_from_slice(&RANDOMX_ARGON_LANES.to_le_bytes());
    // outlen (u32 LE) = 0 (RandomX doesn't use Argon2 output directly)
    input.extend_from_slice(&0u32.to_le_bytes());
    // m_cost (u32 LE)
    input.extend_from_slice(&RANDOMX_ARGON_MEMORY.to_le_bytes());
    // t_cost (u32 LE)
    input.extend_from_slice(&RANDOMX_ARGON_ITERATIONS.to_le_bytes());
    // version (u32 LE)
    input.extend_from_slice(&ARGON2_VERSION.to_le_bytes());
    // type (u32 LE) = 0 (Argon2d)
    input.extend_from_slice(&0u32.to_le_bytes());
    // pwdlen (u32 LE)
    input.extend_from_slice(&(key.len() as u32).to_le_bytes());
    // pwd
    input.extend_from_slice(key);
    // saltlen (u32 LE)
    input.extend_from_slice(&(RANDOMX_ARGON_SALT.len() as u32).to_le_bytes());
    // salt
    input.extend_from_slice(RANDOMX_ARGON_SALT);
    // secretlen (u32 LE) = 0
    input.extend_from_slice(&0u32.to_le_bytes());
    // adlen (u32 LE) = 0
    input.extend_from_slice(&0u32.to_le_bytes());

    let hash = blake2b::blake2b(ARGON2_PREHASH_DIGEST_LENGTH, &input);
    hash.try_into().unwrap()
}

/// Fill first two blocks for each lane from H0.
fn fill_first_blocks(memory: &mut [Block], blockhash: &[u8; ARGON2_PREHASH_DIGEST_LENGTH], lanes: u32, lane_length: u32) {
    let mut seed = [0u8; ARGON2_PREHASH_SEED_LENGTH];
    seed[..ARGON2_PREHASH_DIGEST_LENGTH].copy_from_slice(blockhash);

    for l in 0..lanes {
        // Block 0: G(H0 || 0 || lane)
        seed[ARGON2_PREHASH_DIGEST_LENGTH..ARGON2_PREHASH_DIGEST_LENGTH + 4]
            .copy_from_slice(&0u32.to_le_bytes());
        seed[ARGON2_PREHASH_DIGEST_LENGTH + 4..ARGON2_PREHASH_SEED_LENGTH]
            .copy_from_slice(&l.to_le_bytes());
        let block_bytes = blake2b_long(ARGON2_BLOCK_SIZE, &seed);
        memory[(l * lane_length) as usize].load_from_bytes(&block_bytes);

        // Block 1: G(H0 || 1 || lane)
        seed[ARGON2_PREHASH_DIGEST_LENGTH..ARGON2_PREHASH_DIGEST_LENGTH + 4]
            .copy_from_slice(&1u32.to_le_bytes());
        let block_bytes = blake2b_long(ARGON2_BLOCK_SIZE, &seed);
        memory[(l * lane_length + 1) as usize].load_from_bytes(&block_bytes);
    }
}

/// Initialize the RandomX cache using Argon2d.
/// Returns the cache memory (262144 * 1024 bytes = 256 MiB).
pub fn argon2d_cache(key: &[u8]) -> Vec<u8> {
    let memory_blocks = RANDOMX_ARGON_MEMORY; // Each block = 1 KiB
    let lanes = RANDOMX_ARGON_LANES;
    let segment_length = memory_blocks / (lanes * ARGON2_SYNC_POINTS);
    let lane_length = segment_length * ARGON2_SYNC_POINTS;

    // Allocate memory blocks
    let total_blocks = (lanes * lane_length) as usize;
    let mut memory: Vec<Block> = Vec::with_capacity(total_blocks);
    for _ in 0..total_blocks {
        memory.push(Block::new());
    }

    // Initial hash H0
    let blockhash = initial_hash(key);

    // Fill first blocks
    fill_first_blocks(&mut memory, &blockhash, lanes, lane_length);

    // Fill memory (3 passes, single-threaded)
    for pass in 0..RANDOMX_ARGON_ITERATIONS {
        for slice in 0..ARGON2_SYNC_POINTS {
            for lane in 0..lanes {
                fill_segment(
                    &mut memory,
                    pass,
                    lane,
                    slice as u8,
                    lanes,
                    lane_length,
                    segment_length,
                );
            }
        }
    }

    // Convert blocks to raw bytes
    let total_bytes = total_blocks * ARGON2_BLOCK_SIZE;
    let mut result = vec![0u8; total_bytes];
    for (i, block) in memory.iter().enumerate() {
        block.store_to_bytes(&mut result[i * ARGON2_BLOCK_SIZE..(i + 1) * ARGON2_BLOCK_SIZE]);
    }
    result
}
