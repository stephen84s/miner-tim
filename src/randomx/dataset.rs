// Dataset item computation
// Reference: RandomX src/dataset.cpp

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

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

/// Full dataset: 2 GiB + 32 MiB = 2,181,038,080 bytes
const DATASET_BASE_SIZE: usize = 2_147_483_648;
const DATASET_EXTRA_SIZE: usize = 33_554_432;
const DATASET_TOTAL_SIZE: usize = DATASET_BASE_SIZE + DATASET_EXTRA_SIZE;
pub const DATASET_ITEM_COUNT: usize = DATASET_TOTAL_SIZE / CACHE_LINE_SIZE; // 34,078,720

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

// ============================================================================
// Full dataset (precomputed, ~2 GiB)
// ============================================================================

/// Precomputed full RandomX dataset. Read-only after generation, shared across
/// all mining threads via `Arc`.
pub struct RandomXDataset {
    items: Vec<[u64; 8]>,
}

impl RandomXDataset {
    /// Generate the full dataset from cache using `num_threads` threads.
    /// This allocates ~2 GiB and takes 30-120 seconds depending on CPU.
    pub fn generate(
        cache_memory: &[u8],
        programs: &[SuperscalarProgram; 8],
        num_threads: usize,
    ) -> Self {
        // Caught here rather than as an out-of-bounds slice index inside the
        // spawned workers. `RandomXVm::cache_and_programs()` returns an empty
        // cache for a full-mode VM, so this is the failure a caller gets if
        // they pass one instead of a light-mode VM.
        assert!(
            !cache_memory.is_empty(),
            "dataset generation needs a light-mode VM's Argon2d cache; got an              empty one (a full-mode VM does not build one)"
        );
        let mut items = vec![[0u64; 8]; DATASET_ITEM_COUNT];
        let num_threads = num_threads.max(1);
        let chunk_size = DATASET_ITEM_COUNT.div_ceil(num_threads);

        let progress = Arc::new(AtomicUsize::new(0));

        std::thread::scope(|s| {
            for (thread_idx, chunk) in items.chunks_mut(chunk_size).enumerate() {
                let start_item = thread_idx * chunk_size;
                let progress = progress.clone();
                s.spawn(move || {
                    for (i, slot) in chunk.iter_mut().enumerate() {
                        *slot = init_dataset_item(cache_memory, programs, (start_item + i) as u64);
                        if i % 500_000 == 0 && i > 0 {
                            let done = progress.fetch_add(500_000, Ordering::Relaxed) + 500_000;
                            if thread_idx == 0 {
                                log::info!(
                                    "Dataset generation: {:.1}%",
                                    done as f64 / DATASET_ITEM_COUNT as f64 * 100.0
                                );
                            }
                        }
                    }
                    // Count remaining items in this chunk
                    let remainder = chunk.len() % 500_000;
                    if remainder > 0 {
                        progress.fetch_add(remainder, Ordering::Relaxed);
                    }
                });
            }
        });

        RandomXDataset { items }
    }

    /// Look up a precomputed dataset item by index.
    #[inline(always)]
    pub fn get_item(&self, item_number: u64) -> &[u64; 8] {
        &self.items[item_number as usize]
    }

    /// Raw pointer to the dataset backing store, for the native-loop JIT and
    /// the differential test.
    #[cfg(all(test, target_arch = "aarch64"))]
    pub(crate) fn as_ptr_for_test(&self) -> *const u8 {
        self.items.as_ptr() as *const u8
    }

    /// Raw pointer to the dataset backing store, for prefetch hints and the
    /// native-loop JIT. Both are aarch64-only, so on other targets this would
    /// be dead code.
    #[cfg(target_arch = "aarch64")]
    #[inline(always)]
    pub(crate) fn as_ptr(&self) -> *const u8 {
        self.items.as_ptr() as *const u8
    }
}
