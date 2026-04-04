// Dataset item computation (light mode)
// Reference: RandomX src/dataset.cpp

use super::superscalar::{execute_superscalar, SuperscalarProgram};

const CACHE_LINE_SIZE: usize = 64;
const CACHE_SIZE: usize = 262144 * 1024; // 256 MiB
const CACHE_LINE_MASK: u64 = (CACHE_SIZE / CACHE_LINE_SIZE - 1) as u64;
const CACHE_ACCESSES: usize = 8;

const SUPERSCALAR_MUL0: u64 = 6364136223846793005;
const SUPERSCALAR_ADD: [u64; 7] = [
    9298411001130361340,
    12065312585734608966,
    9306329213124626780,
    5281919268842080866,
    10536153434571861004,
    3398623926847679864,
    9549104520008361294,
];

/// Read a native-endian u64 from cache memory at byte offset.
#[inline(always)]
fn load64_native(memory: &[u8], offset: usize) -> u64 {
    u64::from_ne_bytes(memory[offset..offset + 8].try_into().unwrap())
}

/// Get the mix block offset from a register value.
#[inline(always)]
fn get_mix_block_offset(register_value: u64) -> usize {
    ((register_value & CACHE_LINE_MASK) as usize) * CACHE_LINE_SIZE
}

/// Compute a single dataset item from cache in light mode.
/// Returns 64 bytes (8 × u64).
pub fn init_dataset_item(
    cache_memory: &[u8],
    programs: &[SuperscalarProgram; 8],
    item_number: u64,
) -> [u64; 8] {
    let mut rl = [0u64; 8];
    let mut register_value = item_number;

    rl[0] = (item_number.wrapping_add(1)).wrapping_mul(SUPERSCALAR_MUL0);
    for i in 0..7 {
        rl[i + 1] = rl[0] ^ SUPERSCALAR_ADD[i];
    }

    for i in 0..CACHE_ACCESSES {
        let mix_offset = get_mix_block_offset(register_value);

        execute_superscalar(&mut rl, &programs[i]);

        for q in 0..8 {
            rl[q] ^= load64_native(cache_memory, mix_offset + 8 * q);
        }

        register_value = rl[programs[i].address_register];
    }

    rl
}
