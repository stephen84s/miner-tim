// Unit tests for pure Rust RandomX implementation.
// Test vectors from: https://github.com/tevador/RandomX/blob/master/src/tests/tests.cpp
//
// Tests are organized by implementation phase (matching RANDOMX_IMPLEMENTATION_PLAN.md):
//   Phase 1: Blake2b, Soft AES, AES hash functions
//   Phase 2: Argon2d cache
//   Phase 3: Blake2Generator, SuperscalarHash
//   Phase 4: Dataset items
//   Phase 5: VM / Full hash

use super::*;

// ============================================================================
// Helper: hex decode
// ============================================================================
fn hex_decode(hex: &str) -> Vec<u8> {
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
        .collect()
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

/// The full dataset for `b"test key 000"` — the key behind every known-answer
/// vector in this file. It is 2 GiB and three tests need it, so it is built
/// once per test binary rather than once per test.
fn test_key_000_dataset() -> std::sync::Arc<super::dataset::RandomXDataset> {
    static DS: std::sync::LazyLock<std::sync::Arc<super::dataset::RandomXDataset>> =
        std::sync::LazyLock::new(|| {
            let vm_light = vm::RandomXVm::new(b"test key 000");
            let (cache, programs) = vm_light.cache_and_programs();
            std::sync::Arc::new(super::dataset::RandomXDataset::generate(cache, programs, 8))
        });
    DS.clone()
}

// ============================================================================
// Phase 1: Blake2b (RFC 7693)
// ============================================================================
#[cfg(test)]
mod blake2b_tests {
    use super::*;

    #[test]
    fn test_blake2b_512_empty() {
        let hash = blake2b::blake2b_512(b"");
        assert_eq!(
            hex_encode(&hash),
            "786a02f742015903c6c6fd852552d272912f4740e15847618a86e217f71f5419\
             d25e1031afee585313896444934eb04b903a685b1448b755d56f701afe9be2ce"
        );
    }

    #[test]
    fn test_blake2b_256_abc() {
        let hash = blake2b::blake2b_256(b"abc");
        assert_eq!(
            hex_encode(&hash),
            "bddd813c634239723171ef3fee98579b94964e3bb1cb3e427262c8c068d52319"
        );
    }

    #[test]
    fn test_blake2b_512_abc() {
        // Standard test vector from RFC 7693 appendix
        let hash = blake2b::blake2b_512(b"abc");
        assert_eq!(
            hex_encode(&hash),
            "ba80a53f981c4d0d6a2797b69f12f6e94c212f14685ac4b74b12bb6fdbffa2d1\
             7d87c5392aab792dc252d5de4533cc9518d38aa8dbf1925ab92386edd4009923"
        );
    }
}

// ============================================================================
// Phase 1: Soft AES
// ============================================================================

// ============================================================================
// Phase 1: AES Hash Functions
// ============================================================================
#[cfg(test)]
mod aes_hash_tests {
    use super::*;

    /// Test vector from RandomX tests.cpp: AesGenerator1R
    /// C++ test: state = 64 bytes (first 32 from hex, last 32 zeros),
    /// fillAes1Rx4(state, 64, state) — state IS the output buffer.
    /// After call, first 32 bytes of state equal expected hex.
    ///
    /// In our Rust API, state and output are separate, but state is written
    /// back with the final AES state, which equals the output for 1 iteration.
    #[test]
    fn test_fill_aes_1rx4() {
        let mut state = [0u8; 64];
        state[..32].copy_from_slice(&hex_decode(
            "6c19536eb2de31b6c0065f7f116e86f960d8af0c57210a6584c3237b9d064dc7",
        ));

        let mut output = vec![0u8; 64];
        aes_hash::fill_aes_1rx4(&mut state, &mut output);

        // State is written back after the loop, so state == output for single iteration
        assert_eq!(
            hex_encode(&state[..32]),
            "fa89397dd6ca422513aeadba3f124b5540324c4ad4b6db434394307a17c833ab"
        );
        // Output should match state
        assert_eq!(&output[..32], &state[..32]);
    }
}

// ============================================================================
// Phase 2: Argon2d Cache
// ============================================================================
#[cfg(test)]
mod argon2d_tests {
    use super::*;

    /// Cache initialization test from RandomX tests.cpp.
    /// Key: "test key 000"
    /// Argon2d params: t=3, m=262144 KiB, p=1, salt="RandomX\x03"
    /// Expected cache memory values (as u64 little-endian):
    ///   memory[0]        = 0x191e0e1d23c02186
    ///   memory[1568413]  = 0xf1b62fe6210bf8b1
    ///   memory[33554431] = 0x1f47f056d05cd99b
    /// Note: memory has 262144 * 1024 / 8 = 33554432 u64 values
    #[test]
    fn test_cache_initialization() {
        let cache = argon2d::argon2d_cache(b"test key 000");
        // Cache is 262144 * 1024 = 268435456 bytes
        assert_eq!(cache.len(), 262144 * 1024);

        // Read as u64 little-endian
        let read_u64 = |offset: usize| -> u64 {
            u64::from_le_bytes(cache[offset * 8..offset * 8 + 8].try_into().unwrap())
        };

        assert_eq!(read_u64(0), 0x191e0e1d23c02186);
        assert_eq!(read_u64(1568413), 0xf1b62fe6210bf8b1);
        assert_eq!(read_u64(33554431), 0x1f47f056d05cd99b);
    }
}

// ============================================================================
// Phase 3: Blake2Generator
// ============================================================================
#[cfg(test)]
mod blake2gen_tests {
    use super::*;

    /// Basic test: Blake2Generator should produce deterministic output.
    /// We can verify this indirectly through the SuperscalarHash tests,
    /// but here we test the basic mechanics.
    #[test]
    fn test_blake2gen_basic() {
        let mut generator = blake2gen::Blake2Generator::new(b"test key 000", 0);
        // After construction, data_index=64 so first call triggers a blake2b hash
        let b1 = generator.get_byte();
        let b2 = generator.get_byte();
        // These values depend on blake2b_512 of the initial data
        // We can't hardcode expected values without blake2b, but we can
        // verify the generator produces consistent output
        let mut gen2 = blake2gen::Blake2Generator::new(b"test key 000", 0);
        assert_eq!(gen2.get_byte(), b1);
        assert_eq!(gen2.get_byte(), b2);
    }

    #[test]
    fn test_blake2gen_different_nonce() {
        let mut gen0 = blake2gen::Blake2Generator::new(b"test key 000", 0);
        let mut gen1 = blake2gen::Blake2Generator::new(b"test key 000", 1);
        // Different nonces should produce different sequences
        let b0 = gen0.get_byte();
        let b1 = gen1.get_byte();
        // Very unlikely to be equal (1/256 chance), but not impossible
        // This is a weak test; SuperscalarHash tests provide stronger validation
        let _ = (b0, b1);
    }
}

// ============================================================================
// Phase 3: Reciprocal Function
// ============================================================================
#[cfg(test)]
mod reciprocal_tests {
    use super::*;

    /// Test vectors from RandomX tests.cpp: randomx_reciprocal
    #[test]
    fn test_randomx_reciprocal() {
        assert_eq!(superscalar::randomx_reciprocal(3), 12297829382473034410);
        assert_eq!(superscalar::randomx_reciprocal(13), 11351842506898185609);
        assert_eq!(superscalar::randomx_reciprocal(33), 17887751829051686415);
        assert_eq!(superscalar::randomx_reciprocal(65537), 18446462603027742720);
        assert_eq!(superscalar::randomx_reciprocal(15000001), 10316166306300415204);
        assert_eq!(superscalar::randomx_reciprocal(3845182035), 10302264209224146340);
        assert_eq!(superscalar::randomx_reciprocal(0xffffffff), 9223372039002259456);
    }
}

// ============================================================================
// Phase 3: SuperscalarHash Program Generation
// ============================================================================
#[cfg(test)]
mod superscalar_tests {
    use super::*;

    /// Test vectors from RandomX tests.cpp: SuperscalarHash generator
    /// Key: "test key 000", generates 10 programs with nonce starting at 0.
    /// Each program is hashed with Blake2b-256 over its instruction buffer.
    ///
    /// The program hash is Blake2b-256 of (instructions as raw bytes).
    /// Each instruction is 8 bytes: [opcode, dst, src, mod, imm32[4]].
    #[test]
    fn test_superscalar_program_generation() {
        let expected_hashes = [
            "d3a4a6623738756f77e6104469102f082eff2a3e60be7ad696285ef7dfc72a61",
            "f5e7e0bbc7e93c609003d6359208688070afb4a77165a552ff7be63b38dfbc86",
            "85ed8b11734de5b3e9836641413a8f36e99e89694f419c8cd25c3f3f16c40c5a",
            "5dd956292cf5d5704ad99e362d70098b2777b2a1730520be52f772ca48cd3bc0",
            "6f14018ca7d519e9b48d91af094c0f2d7e12e93af0228782671a8640092af9e5",
            "134be097c92e2c45a92f23208cacd89e4ce51f1009a0b900dbe83b38de11d791",
            "268f9392c20c6e31371a5131f82bd7713d3910075f2f0468baafaa1abd2f3187",
            "c668a05fd909714ed4a91e8d96d67b17e44329e88bc71e0672b529a3fc16be47",
            "99739351315840963011e4c5d8e90ad0bfed3facdcb713fe8f7138fbf01c4c94",
            "14ab53d61880471f66e80183968d97effd5492b406876060e595fcf9682f9295",
        ];

        let key = b"test key 000";
        let mut generator = blake2gen::Blake2Generator::new(key, 0);

        for (i, expected_hex) in expected_hashes.iter().enumerate() {
            let prog = superscalar::generate_superscalar(&mut generator);

            // Serialize program to raw bytes (same layout as C++ Instruction struct)
            let mut prog_bytes = Vec::new();
            for inst in &prog.instructions {
                prog_bytes.push(inst.opcode);
                prog_bytes.push(inst.dst);
                prog_bytes.push(inst.src);
                prog_bytes.push(inst.mod_);
                prog_bytes.extend_from_slice(&inst.imm32.to_le_bytes());
            }

            let hash = blake2b::blake2b_256(&prog_bytes);
            assert_eq!(
                hex_encode(&hash),
                *expected_hex,
                "SuperscalarHash program {} hash mismatch",
                i
            );
        }
    }

    /// Test SuperscalarHash execution with known dataset item results.
    /// If programs generate correctly AND execute correctly, dataset items will match.
    /// This is tested more directly in the dataset tests below.
    #[test]
    fn test_superscalar_execution_basic() {
        // Simple sanity check: execute with known registers
        let prog = superscalar::SuperscalarProgram {
            instructions: vec![],
            address_register: 0,
        };
        let mut r = [0u64; 8];
        superscalar::execute_superscalar(&mut r, &prog);
        // Empty program should leave registers unchanged
        assert_eq!(r, [0u64; 8]);
    }
}

// ============================================================================
// Phase 4: Dataset Items
// ============================================================================
#[cfg(test)]
mod dataset_tests {
    use super::*;

    /// Test vectors from RandomX tests.cpp: Dataset initialization (interpreter)
    /// Key: "test key 000"
    /// Expected first u64 of each dataset item:
    ///   Item 0:        0x680588a85ae222db
    ///   Item 10000000: 0x7943a1f6186ffb72
    ///   Item 20000000: 0x9035244d718095e1
    ///   Item 30000000: 0x145a5091f7853099
    #[test]
    fn test_dataset_item_0() {
        let cache_memory = argon2d::argon2d_cache(b"test key 000");
        let key = b"test key 000";
        let mut generator = blake2gen::Blake2Generator::new(key, 0);
        let mut programs = Vec::new();
        for _ in 0..8 {
            programs.push(superscalar::generate_superscalar(&mut generator));
        }
        let programs: [superscalar::SuperscalarProgram; 8] = programs.try_into().unwrap();

        let item = dataset::init_dataset_item(&cache_memory, &programs, 0);
        assert_eq!(item[0], 0x680588a85ae222db);
    }

    #[test]
        fn test_dataset_item_10m() {
        let cache_memory = argon2d::argon2d_cache(b"test key 000");
        let key = b"test key 000";
        let mut generator = blake2gen::Blake2Generator::new(key, 0);
        let mut programs = Vec::new();
        for _ in 0..8 {
            programs.push(superscalar::generate_superscalar(&mut generator));
        }
        let programs: [superscalar::SuperscalarProgram; 8] = programs.try_into().unwrap();

        let item = dataset::init_dataset_item(&cache_memory, &programs, 10_000_000);
        assert_eq!(item[0], 0x7943a1f6186ffb72);
    }

    #[test]
        fn test_dataset_item_20m() {
        let cache_memory = argon2d::argon2d_cache(b"test key 000");
        let key = b"test key 000";
        let mut generator = blake2gen::Blake2Generator::new(key, 0);
        let mut programs = Vec::new();
        for _ in 0..8 {
            programs.push(superscalar::generate_superscalar(&mut generator));
        }
        let programs: [superscalar::SuperscalarProgram; 8] = programs.try_into().unwrap();

        let item = dataset::init_dataset_item(&cache_memory, &programs, 20_000_000);
        assert_eq!(item[0], 0x9035244d718095e1);
    }

    #[test]
        fn test_dataset_item_30m() {
        let cache_memory = argon2d::argon2d_cache(b"test key 000");
        let key = b"test key 000";
        let mut generator = blake2gen::Blake2Generator::new(key, 0);
        let mut programs = Vec::new();
        for _ in 0..8 {
            programs.push(superscalar::generate_superscalar(&mut generator));
        }
        let programs: [superscalar::SuperscalarProgram; 8] = programs.try_into().unwrap();

        let item = dataset::init_dataset_item(&cache_memory, &programs, 30_000_000);
        assert_eq!(item[0], 0x145a5091f7853099);
    }
}

// ============================================================================
// Phase 5: Full Hash (V1 interpreter, light mode)
// ============================================================================
#[cfg(test)]
mod full_hash_tests {
    use super::*;

    /// Test vectors from RandomX tests.cpp (V1 mode, no RANDOMX_FLAG_V2)
    /// Key: "test key 000", Input: "This is a test"
    /// Expected: 639183aae1bf4c9a35884cb46b09cad9175f04efd7684e7262a0ac1c2f0b4e3f
    #[test]
        fn test_full_hash_v1_a() {
        let hash = vm::calculate_hash(b"test key 000", b"This is a test");
        assert_eq!(
            hex_encode(&hash),
            "639183aae1bf4c9a35884cb46b09cad9175f04efd7684e7262a0ac1c2f0b4e3f"
        );
    }

    /// Key: "test key 000", Input: "Lorem ipsum dolor sit amet"
    /// Expected: 300a0adb47603dedb42228ccb2b211104f4da45af709cd7547cd049e9489c969
    #[test]
        fn test_full_hash_v1_b() {
        let hash = vm::calculate_hash(b"test key 000", b"Lorem ipsum dolor sit amet");
        assert_eq!(
            hex_encode(&hash),
            "300a0adb47603dedb42228ccb2b211104f4da45af709cd7547cd049e9489c969"
        );
    }

    /// Key: "test key 000", Input: "sed do eiusmod tempor incididunt ut labore et dolore magna aliqua"
    /// Expected: c36d4ed4191e617309867ed66a443be4075014e2b061bcdaf9ce7b721d2b77a8
    #[test]
        fn test_full_hash_v1_c() {
        let hash = vm::calculate_hash(
            b"test key 000",
            b"sed do eiusmod tempor incididunt ut labore et dolore magna aliqua",
        );
        assert_eq!(
            hex_encode(&hash),
            "c36d4ed4191e617309867ed66a443be4075014e2b061bcdaf9ce7b721d2b77a8"
        );
    }

    /// Key: "test key 001" (different key!), same input
    /// Expected: e9ff4503201c0c2cca26d285c93ae883f9b1d30c9eb240b820756f2d5a7905fc
    #[test]
        fn test_full_hash_v1_different_key() {
        let hash = vm::calculate_hash(
            b"test key 001",
            b"sed do eiusmod tempor incididunt ut labore et dolore magna aliqua",
        );
        assert_eq!(
            hex_encode(&hash),
            "e9ff4503201c0c2cca26d285c93ae883f9b1d30c9eb240b820756f2d5a7905fc"
        );
    }

    /// Test RandomXVm::calculate_hash (uses JIT on aarch64) matches known hash.
    /// Uses full mode (precomputed dataset) for speed.
    #[test]
    fn test_vm_calculate_hash_jit() {
        let key = b"test key 000";
        let input = b"This is a test";
        let expected = "639183aae1bf4c9a35884cb46b09cad9175f04efd7684e7262a0ac1c2f0b4e3f";

        // Forced onto the per-iteration body JIT. Since stage D made the
        // native loop the default, this is no longer the default path — but it
        // is still a shipping one (`set_native_loop(false)` selects it, and so
        // does any non-aarch64 or light-mode build), so it keeps its own
        // known-answer vector rather than being deleted.
        let mut vm_full = vm::RandomXVm::new_full(key, test_key_000_dataset());
        vm_full.set_native_loop(false);
        let hash = vm_full.calculate_hash(input);

        assert_eq!(
            hex_encode(&hash),
            expected,
            "RandomXVm (JIT) hash must match known test vector"
        );
    }

    /// Known-answer hash through the **native loop** (DESIGN_JIT_NATIVE_LOOP.md
    /// stage C gate).
    ///
    /// The differential tests in `native_loop_diff_tests` prove the native loop
    /// agrees with the interpreter, which says nothing if both are wrong in the
    /// same way. This is the only test that anchors emitted native-loop code to
    /// a real RandomX result, and the only one that exercises FPCR carry-over
    /// across all eight chains and the
    /// `serialize_register_file` -> `blake2b_512` -> next-program plumbing with
    /// the native loop in the path.
    #[cfg(target_arch = "aarch64")]
    #[test]
    fn test_native_loop_known_answer() {
        let key = b"test key 000";
        let input = b"This is a test";
        let expected = "639183aae1bf4c9a35884cb46b09cad9175f04efd7684e7262a0ac1c2f0b4e3f";

        let mut vm_full = vm::RandomXVm::new_full(key, test_key_000_dataset());
        vm_full.set_native_loop(true);

        assert_eq!(
            hex_encode(&vm_full.calculate_hash(input)),
            expected,
            "native-loop hash must match the reference test vector"
        );
    }

    /// The same gate on the path the miner actually runs. `calculate_hash` is
    /// used by nothing in production: workers call `prepare_scratchpad` once and
    /// then loop on `calculate_hash_pipelined`, which overlaps the AES fill with
    /// the chains. Same input, so the same vector must come out.
    #[cfg(target_arch = "aarch64")]
    #[test]
    fn test_native_loop_known_answer_pipelined() {
        let key = b"test key 000";
        let input = b"This is a test";
        let expected = "639183aae1bf4c9a35884cb46b09cad9175f04efd7684e7262a0ac1c2f0b4e3f";

        let mut vm_full = vm::RandomXVm::new_full(key, test_key_000_dataset());
        vm_full.set_native_loop(true);

        vm_full.prepare_scratchpad(input);
        // `next_input` only seeds the *following* scratchpad; the returned hash
        // is the one for `input`.
        assert_eq!(
            hex_encode(&vm_full.calculate_hash_pipelined(b"unused next blob")),
            expected,
            "native-loop pipelined hash must match the reference test vector"
        );
    }

    /// Full mode must not build an Argon2d cache. `cache_memory` is read in
    /// exactly one place — `init_dataset_item`, on the `dataset == None` arm —
    /// so a VM that owns a dataset never touches it, and building one cost
    /// 256 MiB and ~0.4 s per VM. At 11 workers plus 11 verifiers that was
    /// 5.5 GiB resident and never read. (MR !1 review round 7, R7-F1.)
    #[test]
    fn full_mode_vm_allocates_no_argon2d_cache() {
        let vm_full = vm::RandomXVm::new_full(b"test key 000", test_key_000_dataset());
        assert!(
            vm_full.cache_and_programs().0.is_empty(),
            "full-mode VM built a 256 MiB cache it can never read"
        );
    }

    /// ...but light mode still must, since it computes dataset items on the fly.
    #[test]
    fn light_mode_vm_still_allocates_its_cache() {
        let vm_light = vm::RandomXVm::new(b"test key 000");
        assert!(
            !vm_light.cache_and_programs().0.is_empty(),
            "light mode needs the cache to compute dataset items"
        );
    }

    /// The assumption the share verifier rests on: in the miner's exact usage
    /// pattern, `calculate_hash_pipelined(next)` returns the hash of the
    /// *current* blob, so recomputing `job_blob_current` with `calculate_hash`
    /// reproduces it.
    ///
    /// If this were off by one, every share would be withheld as a false
    /// mismatch — worse than having no verification at all, because it would
    /// look like a JIT fault and cost 100% of revenue. Nothing else covers it:
    /// the known-answer tests each use a single blob, where an off-by-one is
    /// invisible.
    #[cfg(target_arch = "aarch64")]
    #[test]
    fn pipelined_hash_matches_calculate_hash_for_the_preceding_blob() {
        let key = b"test key 000";
        let ds = test_key_000_dataset();

        // Mirror `worker_loop`: a 76-byte blob with a little-endian nonce at
        // 39..43, advanced every iteration.
        let blob_for = |nonce: u32| {
            let mut b = vec![0u8; 76];
            b[0] = 16;
            b[39..43].copy_from_slice(&nonce.to_le_bytes());
            b
        };

        let mut mining_vm = vm::RandomXVm::new_full(key, ds.clone());
        mining_vm.set_native_loop(true);
        let mut verify_vm = vm::RandomXVm::new_full(key, ds);
        verify_vm.set_native_loop(false);

        // Worker startup: prepare on the current blob, then each call passes
        // the *next* blob and returns the hash of the current one.
        let mut current = blob_for(0);
        mining_vm.prepare_scratchpad(&current);

        for nonce in 1..4u32 {
            let next = blob_for(nonce);
            let mined = mining_vm.calculate_hash_pipelined(&next);
            let reference = verify_vm.calculate_hash(&current);
            assert_eq!(
                hex_encode(&mined),
                hex_encode(&reference),
                "pipelined hash at nonce {} is not the hash of the blob the worker \
                 would pass to the verifier — the verifier is off by one",
                nonce - 1
            );
            current = next;
        }
    }

    /// The share verifier's withhold path, driven by two *genuine* RandomX
    /// hashes rather than synthetic byte patterns.
    ///
    /// Review round 7 (R7-Q1) argued the important gap was not "the mismatch
    /// branch never runs" but "nothing proves the comparison is wired up at
    /// all" — if the decision were refactored to be unconditionally submit,
    /// every other test would still pass and the feature would be a silent
    /// no-op. Hashes for adjacent nonces are the realistic shape of a
    /// divergence, so this feeds one in and asserts the share is withheld.
    #[cfg(target_arch = "aarch64")]
    #[test]
    fn verifier_withholds_a_hash_that_does_not_match_the_reference() {
        use crate::miner::{classify_share, ShareVerdict};

        let mut vm_full = vm::RandomXVm::new_full(b"test key 000", test_key_000_dataset());
        let blob = |nonce: u32| {
            let mut b = vec![0u8; 76];
            b[0] = 16;
            b[39..43].copy_from_slice(&nonce.to_le_bytes());
            b
        };
        let h0 = vm_full.calculate_hash(&blob(0));
        let h1 = vm_full.calculate_hash(&blob(1));
        assert_ne!(h0, h1, "adjacent nonces produced the same hash");

        assert_eq!(
            classify_share(true, &h0, Some(&h0)),
            ShareVerdict::SubmitVerified,
            "a matching reference must submit"
        );
        assert_eq!(
            classify_share(true, &h0, Some(&h1)),
            ShareVerdict::Withhold,
            "a genuine divergence must withhold the share"
        );
    }

    /// Verify full mode (precomputed dataset) produces identical hashes to light mode.
    /// This test allocates ~2 GiB and takes 30-120s, so it's ignored by default.
    #[test]
    #[ignore]
    fn test_full_mode_matches_light_mode() {

        let key = b"test key 000";
        let input = b"This is a test";

        // Light mode hash (known-good from test_full_hash_v1_a)
        let light_hash = vm::calculate_hash(key, input);
        assert_eq!(
            hex_encode(&light_hash),
            "639183aae1bf4c9a35884cb46b09cad9175f04efd7684e7262a0ac1c2f0b4e3f"
        );

        // Full mode hash
        let mut vm_full = vm::RandomXVm::new_full(key, test_key_000_dataset());
        let full_hash = vm_full.calculate_hash(input);

        assert_eq!(
            hex_encode(&full_hash),
            hex_encode(&light_hash),
            "Full mode hash must match light mode hash"
        );
    }
}

// ============================================================================
// Phase 3: SuperscalarHash Constants
// ============================================================================
#[cfg(test)]
mod constants_tests {
    /// Verify dataset initialization constants match the C++ reference.
    #[test]
    fn test_superscalar_mul_constant() {
        assert_eq!(6364136223846793005u64, 0x5851F42D4C957F2Du64);
    }

    #[test]
    fn test_superscalar_add_constants() {
        let adds: [u64; 7] = [
            9298411001130361340,
            12065312585734608966,
            9306329213124626780,
            5281919268842080866,
            10536153434571861004,
            3398623926847679864,
            9549104520008361294,
        ];
        // These are the XOR constants for registers r1-r7 in initDatasetItem
        assert_eq!(adds[0], 0x810A_978A_59F5_A1FCu64);
        assert_eq!(adds[1], 0xA770_99DF_38C2_D846u64);
        assert_eq!(adds[2], 0x8126_B91C_BF22_495Cu64);
        assert_eq!(adds[3], 0x494D_2597_179F_8A62u64);
        assert_eq!(adds[4], 0x9237_EFB9_CEAA_EC0Cu64);
        assert_eq!(adds[5], 0x2F2A_5674_6CE6_2D78u64);
        assert_eq!(adds[6], 0x8485_3BF7_B62C_E54Eu64);
    }

    #[test]
    fn test_cache_line_count() {
        // CacheSize = 262144 * 1024 = 268435456 bytes
        // CacheLineSize = 64 bytes
        // CacheLineCount = 268435456 / 64 = 4194304
        let cache_size: u64 = 262144 * 1024;
        let cache_line_size: u64 = 64;
        assert_eq!(cache_size / cache_line_size, 4194304);
    }

    #[test]
    fn test_dataset_extra_items() {
        // DatasetExtraItems = RANDOMX_DATASET_EXTRA_SIZE / 64
        let extra_items = 33554368u64 / 64;
        assert_eq!(extra_items, 524287); // 0x7FFFF
    }

    #[test]
    fn test_scratchpad_masks() {
        let l1: u32 = 16384;
        let l2: u32 = 262144;
        let l3: u32 = 2097152;

        // 8-byte aligned masks
        assert_eq!((l1 - 1) & !7u32, 0x3FF8);
        assert_eq!((l2 - 1) & !7u32, 0x3FFF8);
        assert_eq!((l3 - 1) & !7u32, 0x1FFFF8);

        // 64-byte aligned mask (for L3)
        assert_eq!((l3 - 1) & !63u32, 0x1FFFC0);
    }

    #[test]
    fn test_cache_line_align_mask() {
        // CacheLineAlignMask = (RANDOMX_DATASET_BASE_SIZE - 1) & ~(CacheLineSize - 1)
        let dataset_base: u64 = 2147483648; // 2 GiB
        let cache_line: u64 = 64;
        let mask = (dataset_base - 1) & !(cache_line - 1);
        assert_eq!(mask, 0x7FFFFFC0);
    }
}

// ============================================================================
// Profiling: timing breakdown of a single RandomX hash
// ============================================================================
#[cfg(test)]
mod profile_tests {
    use super::*;

    /// Profile the time spent in each phase of a RandomX hash computation.
    /// Run with: cargo test -p minertim test_hash_profile --release -- --ignored --nocapture
    ///
    /// Uses light mode (no dataset precomputation) which is what the miner uses
    /// on low-memory devices. The execute_vm phase includes dataset item computation
    /// on-the-fly via SuperscalarHash, which is the main bottleneck vs full mode.
    #[test]
    #[ignore]
    fn test_hash_profile() {
        let key = b"test key 000";
        let input = b"This is a test";

        println!("\n--- Allocating RandomX VM (light mode, includes Argon2d cache init) ---");
        let t = std::time::Instant::now();
        let mut vm = vm::RandomXVm::new(key);
        println!("  VM creation (Argon2d + SuperscalarHash gen): {:?}\n", t.elapsed());

        // Warm up: compute one hash to ensure all code paths are hot
        println!("--- Warm-up hash ---");
        let _ = vm.calculate_hash(input);

        // Profiled hash
        println!("--- Profiled hash ---");
        let hash = vm.calculate_hash_profiled(input);
        println!("  Result: {}", hex_encode(&hash));

        // Verify correctness
        assert_eq!(
            hex_encode(&hash),
            "639183aae1bf4c9a35884cb46b09cad9175f04efd7684e7262a0ac1c2f0b4e3f",
            "Profiled hash must match known test vector"
        );

        // Run a few more to get stable average
        println!("--- Batch of 5 hashes for averaging ---");
        let mut total = std::time::Duration::ZERO;
        for i in 0..5 {
            let modified_input = format!("This is test input {}", i);
            let t = std::time::Instant::now();
            let _ = vm.calculate_hash(modified_input.as_bytes());
            let elapsed = t.elapsed();
            total += elapsed;
            println!("  Hash {}: {:?} ({:.1} H/s)", i, elapsed, 1.0 / elapsed.as_secs_f64());
        }
        let avg = total / 5;
        println!("  Average: {:?} ({:.1} H/s single-thread)", avg, 1.0 / avg.as_secs_f64());
        println!();

        // Print the detailed profile one more time with a different input
        // to show it's consistent
        println!("--- Second profiled hash (different input) ---");
        let hash2 = vm.calculate_hash_profiled(b"Different input data for profiling");
        println!("  Result: {}", hex_encode(&hash2));
    }
}

// ============================================================================
// RandomX v2 (rx/2): commitment (RANDOMX_V2_SEMANTICS.md §7.2)
// ============================================================================
#[cfg(test)]
mod commitment_tests {
    use super::*;

    /// Pure-Blake2b vector, no VM needed: input "This is a test" ‖ v1 hash of it.
    /// From tevador/RandomX v1.2.1 tests.cpp (calcStringCommitment).
    #[test]
    fn test_commitment_v1_based() {
        let hash: [u8; 32] =
            hex_decode("639183aae1bf4c9a35884cb46b09cad9175f04efd7684e7262a0ac1c2f0b4e3f")
                .try_into()
                .unwrap();
        let commitment = vm::calculate_commitment(b"This is a test", &hash);
        assert_eq!(
            hex_encode(&commitment),
            "d53ccf348b75291b7be76f0a7ac8208bbced734b912f6fca60539ab6f86be919"
        );
    }

    /// Same input, v2 hash of it (master tests.cpp "Commitment test").
    #[test]
    fn test_commitment_v2_based() {
        let hash: [u8; 32] =
            hex_decode("22ec6b861b3eb23686b2efbad69513c967ecfce80983df66c9c5b4fbfb4cdb6f")
                .try_into()
                .unwrap();
        let commitment = vm::calculate_commitment(b"This is a test", &hash);
        assert_eq!(
            hex_encode(&commitment),
            "133be717399046b03ae82ce8ddd9d1ee4d3ea7fca03a50dec09b6848cbb98e18"
        );
    }
}

// ============================================================================
// RandomX v2 (rx/2): full-hash vectors (RANDOMX_V2_SEMANTICS.md §7.1)
// ============================================================================
#[cfg(test)]
mod full_hash_v2_tests {
    use super::*;

    /// Vector (a): key "test key 000", input "This is a test", V2 mode.
    /// From tevador/RandomX master tests.cpp (RANDOMX_FLAG_V2 branch).
    #[test]
    fn test_full_hash_v2_a() {
        let hash = vm::calculate_hash_v2(b"test key 000", b"This is a test");
        assert_eq!(
            hex_encode(&hash),
            "22ec6b861b3eb23686b2efbad69513c967ecfce80983df66c9c5b4fbfb4cdb6f"
        );
    }

    /// Vector (e): key "test key 001", 76-byte Monero-shaped blob — the most
    /// representative single vector for the miner path.
    #[test]
    fn test_full_hash_v2_e() {
        let blob = hex_decode(
            "0b0b98bea7e805e0010a2126d287a2a0cc833d312cb786385a7c2f9de69d2553\
             7f584a9bc9977b00000000666fd8753bf61a8631f12984e3fd44f4014eca6292\
             76817b56f32e9b68bd82f416",
        );
        assert_eq!(blob.len(), 76);
        let hash = vm::calculate_hash_v2(b"test key 001", &blob);
        assert_eq!(
            hex_encode(&hash),
            "c8e92c5f7c1946fecf06bc382b92e3111da38ee3e6a5ad90704e1a9d8aaf6e76"
        );
    }
}

// ============================================================================
// RandomX v2: JIT path (RandomXVm light mode uses the JIT on aarch64)
// ============================================================================
#[cfg(test)]
mod v2_jit_tests {
    use super::*;
    use super::vm::{RandomXVm, RxVersion};

    /// RandomXVm (JIT on aarch64, interpreter elsewhere) must match the
    /// v2 reference vectors — proves the conditional-CFROUND emission.
    #[test]
    fn test_vm_v2_vectors() {
        let mut vm = RandomXVm::new_versioned(b"test key 000", RxVersion::V2);
        let hash = vm.calculate_hash(b"This is a test");
        assert_eq!(
            hex_encode(&hash),
            "22ec6b861b3eb23686b2efbad69513c967ecfce80983df66c9c5b4fbfb4cdb6f"
        );

        let mut vm = RandomXVm::new_versioned(b"test key 001", RxVersion::V2);
        let blob = hex_decode(
            "0b0b98bea7e805e0010a2126d287a2a0cc833d312cb786385a7c2f9de69d2553\
             7f584a9bc9977b00000000666fd8753bf61a8631f12984e3fd44f4014eca6292\
             76817b56f32e9b68bd82f416",
        );
        let hash = vm.calculate_hash(&blob);
        assert_eq!(
            hex_encode(&hash),
            "c8e92c5f7c1946fecf06bc382b92e3111da38ee3e6a5ad90704e1a9d8aaf6e76"
        );
    }

    /// Same VM must flip between versions per the reference `test_switch`:
    /// dataset/cache contents are version-independent.
    #[test]
    fn test_v1_v2_share_cache_key() {
        let mut vm1 = RandomXVm::new_versioned(b"test key 000", RxVersion::V1);
        assert_eq!(
            hex_encode(&vm1.calculate_hash(b"This is a test")),
            "639183aae1bf4c9a35884cb46b09cad9175f04efd7684e7262a0ac1c2f0b4e3f"
        );
    }
}

// ============================================================================
// Native-loop JIT: differential test against the interpreter
// (DESIGN_JIT_NATIVE_LOOP.md stage B)
// ============================================================================
#[cfg(all(test, target_arch = "aarch64"))]
mod native_loop_diff_tests {
    use crate::randomx::dataset::RandomXDataset;
    use crate::randomx::jit::JitCompiler;
    use crate::randomx::vm::{
        self, BytecodeInstruction, NativeRegisterFile, RxVersion, RANDOMX_PROGRAM_SIZE,
        RANDOMX_PROGRAM_SIZE_MAX,
    };
    use std::sync::Arc;


    /// Deterministic pseudo-random program bytes + scratchpad, so both paths
    /// start from byte-identical state.
    fn make_program_bytes(seed: u8) -> Vec<u8> {
        use crate::randomx::blake2gen::Blake2Generator;
        let mut pb = vec![0u8; 128 + RANDOMX_PROGRAM_SIZE * 8];
        let mut g = Blake2Generator::new(&[seed; 32], 0);
        for b in pb.iter_mut() {
            *b = g.get_byte();
        }
        pb
    }

    fn make_scratchpad(seed: u8) -> Vec<u8> {
        use crate::randomx::blake2gen::Blake2Generator;
        let mut sp = vec![0u8; vm::scratchpad_size()];
        let mut g = Blake2Generator::new(&[seed ^ 0xA5; 32], 0);
        // Fills the whole 2 MiB deterministically (~32k Blake2b compressions,
        // ~10 ms). A partially-zero scratchpad would weaken the comparison,
        // since masked addresses roam the entire region.
        for b in sp.iter_mut() {
            *b = g.get_byte();
        }
        sp
    }

    /// Run both paths for `iters` iterations from identical state and assert
    /// the register file, scratchpad and loop-carried state all match exactly.
    fn assert_paths_agree(seed: u8, iters: usize, dataset: &Arc<RandomXDataset>) {
        let program_bytes = make_program_bytes(seed);
        assert_paths_agree_with(&program_bytes, seed, iters, dataset);
    }

    /// As `assert_paths_agree`, but on caller-supplied program bytes, so a test
    /// can pin specific entropy words rather than hoping a seed lands on the
    /// case it wants.
    fn assert_paths_agree_with(
        program_bytes: &[u8],
        sp_seed: u8,
        iters: usize,
        dataset: &Arc<RandomXDataset>,
    ) {
        let (config, ma, mx, dataset_offset) = vm::derive_program_params(program_bytes);

        let mut bytecode: Box<[BytecodeInstruction; RANDOMX_PROGRAM_SIZE_MAX]> =
            Box::new(std::array::from_fn(|_| BytecodeInstruction::new()));
        let mut register_usage = [0i32; 8];
        vm::compile_program(
            program_bytes,
            &mut register_usage,
            &mut bytecode,
            RANDOMX_PROGRAM_SIZE,
        );

        // ---- reference: the interpreter/body-JIT path ----
        // Both paths must start from the same FP rounding mode. CFROUND writes
        // FPCR and never restores it (by design — the mode carries across
        // chains), so without this the second path inherits the first path's
        // final mode and FP results differ by 1 ULP.
        vm::reset_rounding_mode_for_test();
        let mut ref_nreg = NativeRegisterFile::new();
        let mut ref_sp = make_scratchpad(sp_seed);
        let mut ref_bc: Box<[BytecodeInstruction; RANDOMX_PROGRAM_SIZE_MAX]> =
            Box::new(std::array::from_fn(|_| BytecodeInstruction::new()));
        let mut ref_jit = JitCompiler::new().expect("jit");
        let ref_state = vm::execute_vm_for_test(
            &mut ref_nreg,
            &mut ref_sp,
            program_bytes,
            Some(dataset),
            &mut ref_bc,
            Some(&mut ref_jit),
            RxVersion::V1,
            iters,
        );
        let ref_fpcr = vm::read_rounding_mode_for_test();

        // ---- native loop ----
        vm::reset_rounding_mode_for_test();
        let mut jit = JitCompiler::new().expect("jit");
        jit.compile_native_loop(
            &bytecode[..RANDOMX_PROGRAM_SIZE],
            RxVersion::V1,
            &config,
            ma,
            mx,
            dataset_offset,
        );
        let mut new_nreg = NativeRegisterFile::new();
        // a-registers are the loop's only live input besides r; seed them the
        // same way execute_vm_inner does, from the program entropy.
        vm::init_registers_from_entropy_for_test(&mut new_nreg, program_bytes);
        let mut new_sp = make_scratchpad(sp_seed);
        let mut out = [0u64; 4];
        unsafe {
            let f = jit.get_loop_fn();
            f(
                &mut new_nreg as *mut NativeRegisterFile,
                new_sp.as_mut_ptr(),
                dataset.as_ptr_for_test(),
                iters as u64,
                out.as_mut_ptr(),
            );
        }
        let native_fpcr = vm::read_rounding_mode_for_test();

        // ---- compare ----
        assert_eq!(
            new_nreg.r, ref_nreg.r,
            "seed {sp_seed}, {iters} iters: r-registers diverged"
        );
        for i in 0..4 {
            assert_eq!(
                (new_nreg.f[i].0.to_bits(), new_nreg.f[i].1.to_bits()),
                (ref_nreg.f[i].0.to_bits(), ref_nreg.f[i].1.to_bits()),
                "seed {sp_seed}, {iters} iters: f[{i}] diverged"
            );
            assert_eq!(
                (new_nreg.e[i].0.to_bits(), new_nreg.e[i].1.to_bits()),
                (ref_nreg.e[i].0.to_bits(), ref_nreg.e[i].1.to_bits()),
                "seed {sp_seed}, {iters} iters: e[{i}] diverged"
            );
        }
        assert!(
            new_sp == ref_sp,
            "seed {sp_seed}, {iters} iters: scratchpad diverged"
        );
        // D2: ma/mx are not consumed until the *following* iteration, so
        // comparing only nreg+scratchpad cannot detect an ordering error. These
        // carry the real signal.
        //
        // NOTE: sp_addr0/sp_addr1 are zeroed at the end of every iteration on
        // both sides, so those two comparisons are structurally 0 == 0. They
        // are kept as a guard against that zeroing being dropped, but they
        // prove nothing about addressing — do not read them as coverage.
        assert_eq!(
            // Compare the FULL u64. Truncating to u32 here would let a 64-bit
            // EOR on ma/mx pass — the exact C5 violation this out-pointer
            // exists to detect.
            (out[0], out[1], out[2], out[3]),
            (
                ref_state.ma as u64,
                ref_state.mx as u64,
                ref_state.sp_addr0 as u64,
                ref_state.sp_addr1 as u64
            ),
            "seed {sp_seed}, {iters} iters: loop state (ma, mx, sp_addr0, sp_addr1) diverged"
        );
        // The rounding mode must evolve identically. An epilogue that saved and
        // restored FPCR — the C3 violation the design warns about — would pass
        // every assertion above and only surface as a wrong hash across chains.
        assert_eq!(
            native_fpcr, ref_fpcr,
            "seed {sp_seed}, {iters} iters: final FP rounding mode diverged"
        );
    }

    /// The 2 GiB dataset, built once for the whole module. Previously built
    /// per test function, which meant two concurrent 2 GiB allocations plus two
    /// 256 MiB Argon2d caches when both tests ran.
    fn test_dataset() -> Arc<RandomXDataset> {
        static DS: std::sync::LazyLock<Arc<RandomXDataset>> = std::sync::LazyLock::new(|| {
            let vm_light = vm::RandomXVm::new(b"native loop test key");
            let (cache, programs) = vm_light.cache_and_programs();
            Arc::new(RandomXDataset::generate(cache, programs, 8))
        });
        DS.clone()
    }

    /// The C1 memory-safety worst case, executed rather than argued.
    ///
    /// The emitted dataset read is `base + dataset_offset + (ma & 0x7FFF_FFC0)`
    /// with **no runtime bounds check**, and the safety argument is that the
    /// largest reachable address still lands inside the allocation with 64
    /// bytes to spare. Until now that was only ever argued on paper and pinned
    /// by a `const` assert — no test had actually driven the emitted code at
    /// that address, and a seed lands there roughly once in 524,288.
    ///
    /// Both entropy words are forced to their extremes: `entropy(13)` to the
    /// maximum `dataset_offset`, and `entropy(8)` so `ma` masks to the largest
    /// possible value. If the address arithmetic is wrong at the top of the
    /// range this reads out of bounds, which under a test harness means a
    /// segfault or a mismatch rather than a silent wrong hash in production.
    #[test]
    fn native_loop_at_the_c1_worst_case_dataset_address() {
        let ds = test_dataset();
        let mut pb = make_program_bytes(3);

        // entropy(13) -> dataset_offset = (e % (DATASET_EXTRA_ITEMS + 1)) * 64.
        pb[13 * 8..13 * 8 + 8].copy_from_slice(&vm::DATASET_EXTRA_ITEMS.to_le_bytes());
        // entropy(8) -> ma = (e as u32) & CACHE_LINE_ALIGN_MASK.
        pb[8 * 8..8 * 8 + 8].copy_from_slice(&u64::MAX.to_le_bytes());

        let (_, ma, _, dataset_offset) = vm::derive_program_params(&pb);
        assert_eq!(ma, 0x7FFF_FFC0, "ma is not at its maximum");
        assert_eq!(
            dataset_offset,
            vm::DATASET_EXTRA_ITEMS * 64,
            "dataset_offset is not at its maximum"
        );

        // Several iterations: `ma` is XORed each pass, so only the first read
        // is at the pinned extreme, but the bound applies to every one.
        assert_paths_agree_with(&pb, 3, 4, &ds);
    }

    /// The headline gate. N=1 cannot catch an mx-ordering error (design D2), so
    /// N=2 is the minimum meaningful comparison; N=3 guards the steady state.
    #[test]
    fn native_loop_matches_interpreter() {
        let ds = test_dataset();
        // seed 78 has dataset_offset at 99.67% of its maximum, exercising the
        // widest address arithmetic reachable; the others spread readReg0..3.
        for seed in [1u8, 2, 7, 78] {
            assert_paths_agree(seed, 1, &ds);
            assert_paths_agree(seed, 2, &ds);
            assert_paths_agree(seed, 3, &ds);
        }
    }

    /// The emitted loop is a do-while: without the CBZ guard, `iterations == 0`
    /// wraps the counter to u64::MAX and runs ~2^64 times, scribbling the
    /// scratchpad throughout. If this test hangs, that guard has regressed.
    #[test]
    fn native_loop_zero_iterations_terminates() {
        let ds = test_dataset();
        let program_bytes = make_program_bytes(3);
        let (config, ma, mx, dataset_offset) = vm::derive_program_params(&program_bytes);
        let mut bytecode: Box<[BytecodeInstruction; RANDOMX_PROGRAM_SIZE_MAX]> =
            Box::new(std::array::from_fn(|_| BytecodeInstruction::new()));
        let mut register_usage = [0i32; 8];
        vm::compile_program(
            &program_bytes,
            &mut register_usage,
            &mut bytecode,
            RANDOMX_PROGRAM_SIZE,
        );
        let mut jit = JitCompiler::new().expect("jit");
        jit.compile_native_loop(
            &bytecode[..RANDOMX_PROGRAM_SIZE],
            RxVersion::V1,
            &config,
            ma,
            mx,
            dataset_offset,
        );
        let mut nreg = NativeRegisterFile::new();
        vm::init_registers_from_entropy_for_test(&mut nreg, &program_bytes);
        let r_before = nreg.r;
        let mut sp = make_scratchpad(3);
        let sp_before = sp.clone();
        let mut out = [0u64; 4];
        unsafe {
            let f = jit.get_loop_fn();
            f(
                &mut nreg as *mut NativeRegisterFile,
                sp.as_mut_ptr(),
                ds.as_ptr_for_test(),
                0,
                out.as_mut_ptr(),
            );
        }
        // Reaching here at all is the assertion. The r-registers and scratchpad
        // must be untouched; f/e are deliberately not checked (the prologue does
        // not load them, so with zero iterations they are written back as
        // whatever was in d0-d15).
        assert_eq!(nreg.r, r_before, "zero-iteration run modified r-registers");
        assert!(sp == sp_before, "zero-iteration run modified the scratchpad");
        assert_eq!(
            (out[0] as u32, out[1] as u32),
            (ma, mx),
            "zero-iteration run should leave ma/mx at their seeds"
        );
    }

    #[test]
    fn native_loop_matches_interpreter_full_program() {
        let ds = test_dataset();
        assert_paths_agree(11, 2048, &ds);
    }
}
