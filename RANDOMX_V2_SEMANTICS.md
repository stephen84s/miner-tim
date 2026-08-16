# RandomX v2 — Exact Semantics (v1 → v2 delta)

**Compiled:** 2026-08-15
**Purpose:** Implementable reference for MinerTim's gated `rx/2` support. Every change is quoted
verbatim from primary sources so no re-research is needed. Companion to `PLAN_RANDOMX_V2.md`.

**Primary sources** (all quotes below were taken from these exact files on 2026-08-15):

- tevador/RandomX **master** (post-merge of PR #317, "RandomX v2", merged Jan 2026 — supersedes the
  earlier proposal PR #274, which is closed and redirects to #317):
  - https://raw.githubusercontent.com/tevador/RandomX/master/src/configuration.h
  - https://raw.githubusercontent.com/tevador/RandomX/master/src/vm_interpreted.cpp
  - https://raw.githubusercontent.com/tevador/RandomX/master/src/bytecode_machine.hpp
  - https://raw.githubusercontent.com/tevador/RandomX/master/src/program.hpp
  - https://raw.githubusercontent.com/tevador/RandomX/master/src/randomx.h / randomx.cpp
  - https://raw.githubusercontent.com/tevador/RandomX/master/src/soft_aes.h
  - https://raw.githubusercontent.com/tevador/RandomX/master/src/tests/tests.cpp
  - https://raw.githubusercontent.com/tevador/RandomX/master/doc/specs.md
  - https://raw.githubusercontent.com/tevador/RandomX/master/doc/design_v2.md
- baseline for diffing: the same paths at tag **v1.2.1**
  (`https://raw.githubusercontent.com/tevador/RandomX/v1.2.1/src/...`)
- xmrig **v6.26.0** (`https://raw.githubusercontent.com/xmrig/xmrig/v6.26.0/...`) and PR diffs
  `https://github.com/xmrig/xmrig/pull/{3769,3775,3778}.diff`

**Method note:** every tevador file listed under §6 was byte-diffed v1.2.1 → master locally; claims
of "unchanged" below come from those diffs, not from reading release notes.

---

## 0. Summary of ALL consensus-relevant changes

RandomX v2 changes exactly **five** things relative to v1. Everything else is bit-identical.

| # | Change | Where |
|---|--------|-------|
| 1 | Program size 256 → 384 instructions | `configuration.h`, `program.hpp` |
| 2 | CFROUND becomes conditional (writes `fprc` only if bits 2–5 of rotated source are 0 → 1/16 chance) | `bytecode_machine.hpp` |
| 3 | F/E register mixing: XOR → 4 rounds of AES (e-registers are the round keys) | `vm_interpreted.cpp` |
| 4 | Dataset prefetch two iterations ahead (`mp` aliases `ma` instead of `mx`) | `vm_interpreted.cpp` |
| 5 | Commitment: `blake2b_256(input ‖ hash)` is the value compared against the target | `randomx.cpp` (fn existed since v1.2.x; *use* is new) |

xmrig models these as four runtime flags plus the size (PR #3769,
`src/crypto/randomx/randomx.cpp`):

```cpp
RandomX_ConfigurationMoneroV2::RandomX_ConfigurationMoneroV2()
{
	ProgramSize = 384;

	Tweak_V2_CFROUND = 1;
	Tweak_V2_AES = 1;
	Tweak_V2_PREFETCH = 1;
	Tweak_V2_COMMITMENT = 1;
}
```
(https://github.com/xmrig/xmrig/pull/3769.diff)

tevador's reference selects v2 with a new API flag (`src/randomx.h`, master):

```c
  RANDOMX_FLAG_ARGON2 = 96,
  RANDOMX_FLAG_V2 = 128,
```

Note the PR history: **#274 originally proposed only the CFROUND and AES tweaks.** The program-size
increase (384) and the two-iteration prefetch were added during review and landed in **#317**;
master is the authority. (https://github.com/tevador/RandomX/pull/274,
https://github.com/tevador/RandomX/pull/317)

---

## 1. Constants (`configuration.h`)

Verbatim from master `src/configuration.h`; the **only** delta vs v1.2.1 is the program-size block:

```c
//Number of instructions in a RandomX program. Must be divisible by 8.
#define RANDOMX_PROGRAM_SIZE_V1    256
#define RANDOMX_PROGRAM_SIZE_V2    384

#define RANDOMX_PROGRAM_MAX_SIZE   384
```

(v1.2.1 had `#define RANDOMX_PROGRAM_SIZE 256` here; nothing else in the file differs.)

**Explicitly unchanged** (identical text in both files):

```c
#define RANDOMX_ARGON_MEMORY       262144
#define RANDOMX_ARGON_ITERATIONS   3
#define RANDOMX_ARGON_LANES        1
#define RANDOMX_ARGON_SALT         "RandomX\x03"
#define RANDOMX_CACHE_ACCESSES     8
#define RANDOMX_SUPERSCALAR_LATENCY   170
#define RANDOMX_DATASET_BASE_SIZE  2147483648
#define RANDOMX_DATASET_EXTRA_SIZE 33554368
#define RANDOMX_PROGRAM_ITERATIONS 2048
#define RANDOMX_PROGRAM_COUNT      8
#define RANDOMX_SCRATCHPAD_L3      2097152
#define RANDOMX_SCRATCHPAD_L2      262144
#define RANDOMX_SCRATCHPAD_L1      16384
#define RANDOMX_JUMP_BITS          8
#define RANDOMX_JUMP_OFFSET        8
```

All 29 instruction frequencies (`RANDOMX_FREQ_*`) are unchanged — the opcode→instruction mapping
is identical; v2 programs simply contain 384 instructions drawn from the same distribution.

### Program size selection and program bytes

`src/program.hpp` (master):

```cpp
	static uint32_t getSize(randomx_flags flags) {
		return (flags & RANDOMX_FLAG_V2) ? RANDOMX_PROGRAM_SIZE_V2 : RANDOMX_PROGRAM_SIZE_V1;
	}
...
	uint64_t entropyBuffer[16];
	Instruction programBuffer[RANDOMX_PROGRAM_MAX_SIZE];
```

Program bytes generated per program: `128 + 8 * program_size` → **3200 bytes for v2** (2176 for
v1), still produced by the unchanged `AesGenerator4R` from the same 64-byte seed
(`VmBase::generateProgram` in `src/virtual_machine.cpp`: `fillAes4Rx4<softAes>(seed,
sizeof(program), &program);` — note the reference now always generates 3200 bytes and simply
ignores instructions 256..383 in v1 mode; this is observationally equivalent because
`fillAes4Rx4` is a prefix-stable stream and **does not write its state back** — verified in master
`aes_hash.cpp`: `fillAes1Rx4` ends with four `rx_store_vec_i128((rx_vec_i128*)state + i, ...)`
stores, `fillAes4Rx4` has none. So MinerTim may generate exactly 2176 or 3200 bytes per version;
both are correct.)

The 128-byte entropy prefix layout (a-registers, `ma`, `mx`, address registers, `datasetOffset`,
`eMask`) is **byte-identical** to v1 — `randomx_vm::initialize()` in `virtual_machine.cpp` has no
v1/v2 branch (full function diffed clean against v1.2.1).

For MinerTim this means: `PROGRAM_BYTES_SIZE` becomes version-dependent
(`16*8 + program_size*8`), and the bytecode array becomes `[BytecodeInstruction; 384]`
(reference does exactly this: `InstructionByteCode bytecode[RANDOMX_PROGRAM_MAX_SIZE];` in
`vm_interpreted.hpp` — plan Option C confirmed as upstream's own choice).

---

## 2. Conditional CFROUND — the exact rule

**Rule:** rotate the source register right by `imm & 63` **first**; if bits 2–5 of the *rotated*
value are all zero (`(rotated & 60) == 0`, i.e. probability 4/64 · … = **1/16** for uniform input),
write `rotated % 4` into `fprc`; otherwise do nothing. The check is on the rotated value, the
written value is bits 0–1 of the same rotated value.

`src/bytecode_machine.hpp` (master), verbatim:

```cpp
		static void exe_CFROUND(RANDOMX_EXE_ARGS) {
			uint64_t isrc = rotr(*ibc.isrc, ibc.imm);
			if (((flags & RANDOMX_FLAG_V2) == 0) || ((isrc & 60) == 0)) {
				rx_set_rounding_mode(isrc % 4);
			}
		}
```

Bytecode generation is unchanged (`ibc.imm = instr.getImm32() & 63;` — same as v1; confirmed in
master `bytecode_machine.cpp`).

Spec text (`doc/specs.md` §5.4.1):

> This instruction calculates a 2-bit value by rotating the source register right by `imm32` bits
> and taking the 2 least significant bits (the value of the source register is unaffected).
> - **RandomX v1**: bits 0-1 of the result are stored in the `fprc` register.
> - **RandomX v2**: if bits 2-5 of the result are 0, bits 0-1 of the result are stored in the
>   `fprc` register

xmrig interpreter is identical (`0x3C == 60`); xmrig's **ARM64 JIT** emits exactly (v6.26.0
`src/crypto/randomx/jit_compiler_a64.cpp`, `h_CFROUND`):

```cpp
	// ror tmp_reg, src, imm
	emit32(ARMV8A::ROR_IMM | tmp_reg | (src << 5) | ((instr.getImm32() & 63) << 10) | (src << 16), code, k);

	if (RandomX_CurrentConfig.Tweak_V2_CFROUND) {
		// tst tmp_reg, 60
		emit32(0xF27E0E9F, code, k);

		// bne next
		emit32(0x54000081, code, k);
	}

	// bfi fpcr_tmp_reg, tmp_reg, 40, 2
	// rbit tmp_reg, fpcr_tmp_reg
	// msr fpcr, tmp_reg
```

i.e. our `emit_cfround` needs only two extra instructions in v2 mode: `TST tmp, #0x3C` +
`B.NE skip` around the existing FPCR write. (`0xF27E0E9F` is `tst x20, #0x3c`; the `bne` skips 4
instructions.)

**Design intent** (`doc/design_v2.md` §1): switching the x86 MXCSR/FPCR every iteration costs "up
to 10% of hashrate on Ryzen CPUs"; v2 changes the mode "only every 16th time it executes (on
average)". Note the *instruction frequency* (1/256) is unchanged — only the write becomes
conditional.

---

## 3. AES F/E-register mixing (replaces `f[i] ^= e[i]`)

**Where in the loop:** exactly where v1 did `f[i] ^= e[i]` — i.e. loop step 10: *after* the
integer registers were stored to `spAddr1`, *before* the f-registers are stored to `spAddr0`.

**Keys:** the current **e-register values themselves**, bitcast from f64x2 to i128 — no key
schedule, no derivation. The e-registers at this point hold whatever the program left in them
(they were exponent/mantissa-masked at load in step 3 and then modified by FP instructions);
they are used as-is and are **not modified** by the mixing.

**Operation:** 4 single AES rounds per f-register. Register index parity decides direction:
f0/f2 get **encrypt** rounds, f1/f3 get **decrypt** rounds. Round *i* uses key `e[i]`, applied to
all four f-registers, for i = 0,1,2,3 in order.

`src/vm_interpreted.cpp` (master), verbatim — this is the full v2 branch:

```cpp
			if (randomx_vm::getFlags() & RANDOMX_FLAG_V2) {
				rx_vec_i128 ekey[RegisterCountFlt];
				rx_vec_i128 freg[RegisterCountFlt];

				for (unsigned i = 0; i < RegisterCountFlt; ++i) {
					ekey[i] = rx_cast_vec_f2i(nreg.e[i]);
					freg[i] = rx_cast_vec_f2i(nreg.f[i]);
				}

				for (unsigned i = 0; i < RegisterCountFlt; ++i) {
					freg[0] = aesenc<softAes>(freg[0], ekey[i]);
					freg[1] = aesdec<softAes>(freg[1], ekey[i]);
					freg[2] = aesenc<softAes>(freg[2], ekey[i]);
					freg[3] = aesdec<softAes>(freg[3], ekey[i]);
				}

				for (unsigned i = 0; i < RegisterCountFlt; ++i)
					nreg.f[i] = rx_cast_vec_i2f(freg[i]);
			}
			else {
				for (unsigned i = 0; i < RegisterCountFlt; ++i)
					nreg.f[i] = rx_xor_vec_f128(nreg.f[i], nreg.e[i]);
			}
```

Spec text (`doc/specs.md` §4.6.2 step 10):

> - **RandomX v2:** `f0 = AES encrypt of f0 with e0 as key`, `f1 = AES decrypt of f1 with e0 as
>   key`, `f2 = AES encrypt of f2 with e0 as key`, `f3 = AES decrypt of f3 with e0 as key`. These
>   steps are repeated with `e1`, `e2`, `e3` as keys.

**AES round definition** — the same one MinerTim's `soft_aes.rs` already implements for
`AesGenerator1R`/`AesHash1R` (`doc/specs.md` §3.1):

> **AES encryption round** refers to the application of the ShiftRows, SubBytes and MixColumns
> transformations followed by a XOR with the round key.
> **AES decryption round** refers to the application of inverse ShiftRows, inverse SubBytes and
> inverse MixColumns transformations followed by a XOR with the round key.

`aesenc`/`aesdec` in the quote are the same template used by `aes_hash.cpp` (`src/soft_aes.h`):

```cpp
template<bool soft>
inline rx_vec_i128 aesenc(rx_vec_i128 in, rx_vec_i128 key) {
	return soft ? soft_aesenc(in, key) : rx_aesenc_vec_i128(in, key);
}
```

so **MinerTim can reuse `soft_aes::aes_round_enc/dec` (x86 `aesenc`/`aesdec` semantics) directly**.
`soft_aes.cpp` v1.2.1 → master differs only cosmetically (LUTs renamed/exported for asm access);
the round math is unchanged.

**Consequences to be careful about in Rust:**

- After mixing, `f0..f3` are arbitrary 128-bit patterns — they can encode NaN/Inf as doubles
  (`doc/design.md` now says "About 2% (6.85% for RandomX v2)" of scratchpad FP loads hit the
  infinity mask path). Keep f-registers as `u128`/`[u64; 2]` bits through this step, exactly like
  our existing FSCAL handling; never round-trip through `f64` arithmetic.
- The mixed f values are (a) stored to scratchpad at `spAddr0` and (b) remain live in the register
  file — after the last iteration they are part of the 256-byte `RegisterFile` that gets
  blake2b-hashed for chaining/finalisation. Same dataflow as v1's XOR result.
- e-registers are unchanged by the mixing and are stored to the register file as usual at program
  end.

**ARM64 mapping** (for our aarch64 JIT; from xmrig v6.26.0
`src/crypto/randomx/jit_compiler_a64_static.S`, `randomx_program_aarch64_v2_FE_mix`, where
f0..f3 = v16..v19, e0..e3 = v20..v23, and **v28 is zeroed** by the JIT patch `movi v28.4s, 0`):

```asm
	# f0 = aesenc(f0, e0), f1 = aesdec(f1, e0), f2 = aesenc(f2, e0), f3 = aesdec(f3, e0)

	aese	v16.16b, v28.16b
	aesd	v17.16b, v28.16b
	aese	v18.16b, v28.16b
	aesd	v19.16b, v28.16b

	aesmc	v16.16b, v16.16b
	aesimc	v17.16b, v17.16b
	aesmc	v18.16b, v18.16b
	aesimc	v19.16b, v19.16b

	eor	v16.16b, v16.16b, v20.16b
	eor	v17.16b, v17.16b, v20.16b
	eor	v18.16b, v18.16b, v20.16b
	eor	v19.16b, v19.16b, v20.16b
	# ...same block repeated with v21, v22, v23 as the eor operand
```

i.e. x86 `aesenc(state, key)` ≡ ARM `AESE(state, 0); AESMC; EOR key` and
`aesdec(state, key)` ≡ `AESD(state, 0); AESIMC; EOR key` (ARMv8 folds AddRoundKey *before*
SubBytes/ShiftRows, hence the zero register). 12 instructions per round-group, 48 total.

**Design intent** (`doc/design_v2.md` §2): doubles AES work per hash "without hurting the
hashrate (it uses the gap in RandomX main loop where the CPU was sitting idle, waiting for
scratchpad data)", forces ASICs to put AES inside the VM, and improves entropy of scratchpad
writes.

---## 4. Prefetch change (`mp` aliasing) — the change the plan file missed

v2 also changes **loop steps 5–8** (dataset addressing). This is consensus-relevant: it changes
*which register accumulates the address entropy*, and therefore which dataset lines are read.

`src/vm_interpreted.cpp` (master), verbatim (full loop tail):

```cpp
			executeBytecode(bytecode, scratchpad, config, randomx_vm::getFlags());

			const uint64_t readPtr = datasetOffset + (mem.ma & CacheLineAlignMask);

			auto& mp = (randomx_vm::getFlags() & RANDOMX_FLAG_V2) ? mem.ma : mem.mx;
			mp ^= nreg.r[config.readReg2] ^ nreg.r[config.readReg3];

			datasetPrefetch(datasetOffset + (mp & CacheLineAlignMask));
			datasetRead(readPtr, nreg.r);
			std::swap(mem.mx, mem.ma);
```

Semantics per iteration (`ma`/`mx` are `uint32_t`; the XOR takes the low 32 bits of
`r[readReg2] ^ r[readReg3]`):

- **v1** (identical to what MinerTim does today): read address = old `ma`; `mx ^= spMix2`;
  prefetch new `mx`; swap. (The reference refactored v1 from "mask `mx` in place" to "mask at
  use"; this is **bit-equivalent** because XOR distributes over the AND mask —
  `((x & m) ^ v) & m == (x ^ v) & m`. Verified against v1.2.1 which had
  `mem.mx &= CacheLineAlignMask;` in place.)
- **v2**: read address = old `ma` (captured in `readPtr` **before** the XOR); then **`ma`** (not
  `mx`) absorbs the XOR; prefetch the *new* `ma`; swap. The freshly prefetched address sits in
  `mx` for one full iteration and reaches `ma` (the read register) only after the *next* swap →
  the prefetch runs **two iterations** ahead of its read.

Spec formulation (`doc/specs.md` §4.6.2 steps 5–8, using the new `mt` helper register and the
alias "RandomX v1: `mp` is a name alias for `mx` / RandomX v2: `mp` is a name alias for `ma`"
from §4.3):

> 5. The value of `ma` is saved in `mt`. Then the `mp` register is XORed with the low 32 bits of
>    registers `readReg2` and `readReg3` (see Table 4.5.3).
> 6. A 64-byte Dataset item at address `datasetOffset + mp % RANDOMX_DATASET_BASE_SIZE` is
>    prefetched from the Dataset (it will be used during the next iteration(s)).
> 7. A 64-byte Dataset item at address `datasetOffset + mt % RANDOMX_DATASET_BASE_SIZE` is loaded
>    from the Dataset. The 64 bytes are XORed with all integer registers in order `r0`-`r7`.
> 8. The values of registers `mx` and `ma` are swapped.

(`mt` is just the interpreter's `readPtr` temporary; no new persistent state.)

Initialisation is unchanged: `mem.ma = entropy(8) & CacheLineAlignMask; mem.mx = entropy(10);`
and `spAddr0 = mx; spAddr1 = ma;` at program start, `datasetOffset = (entropy(13) %
(DatasetExtraItems + 1)) * 64` — all identical to v1 (diffed clean).

xmrig's ARM64 rendering makes the delta vivid (v6.26.0 `jit_compiler_a64_static.S`; x9 packs
mx in the low 32 bits and ma in the high 32, w20 = `readReg2 ^ readReg3`; the JIT copies one of
these 16-byte blocks over the loop tail):

```asm
DECL(randomx_program_aarch64_vm_instructions_end_v1):
	lsr	x10, x9, 32      # read addr = ma
	eor	x9, x9, x20      # mx ^= spMix2
	mov	w20, w9          # prefetch addr = new mx
	ror	x9, x9, 32       # swap mx <-> ma

DECL(randomx_program_aarch64_vm_instructions_end_v2):
	lsr	x10, x9, 32      # read addr = ma (old)
	ror	x9, x9, 32       # swap first: ma now in low half
	eor	x9, x9, x20      # ma ^= spMix2
	mov	w20, w9          # prefetch addr = new ma
```

**MinerTim mapping** (`src/randomx/vm.rs` ~line 1143): our loop already computes
`read_ptr = dataset_offset + (mem_ma & CACHE_LINE_ALIGN_MASK)` before updating; the v2 change is
literally: XOR `spMix2` into `mem_ma` instead of `mem_mx` (then the existing swap + prefetch order
must match the reference: prefetch the post-XOR `mp` value *before* the swap).

Rationale (`doc/design_v2.md` §4): "RandomX v1 prefetches data from the dataset one iteration
ahead. RandomX v2 increases it to two iterations by redefining the prefetch logic."

---

## 5. Commitment

### 5.1 The function

`src/randomx.cpp` (master; **identical code already exists in v1.2.1** — added by
tevador/RandomX#265; `RANDOMX_HASH_SIZE` = 32):

```cpp
	void randomx_calculate_commitment(const void* input, size_t inputSize, const void* hash_in, void* com_out) {
		assert(inputSize == 0 || input != nullptr);
		assert(hash_in != nullptr);
		assert(com_out != nullptr);
		blake2b_state state;
		blake2b_init(&state, RANDOMX_HASH_SIZE);
		blake2b_update(&state, input, inputSize);
		blake2b_update(&state, hash_in, RANDOMX_HASH_SIZE);
		blake2b_final(&state, com_out, RANDOMX_HASH_SIZE);
	}
```

So: **plain Blake2b, 32-byte digest, no key, no salt/personalisation**, input = the full hashing
blob (same bytes that went into `randomx_calculate_hash`, i.e. the nonced 76-byte hashing blob in
Monero's case) followed by the 32-byte RandomX hash. Exactly the plan's Phase-3 sketch; our
`blake2b_256` fits as-is.

### 5.2 When it's computed, what is compared to the target, and what goes on the wire

From xmrig v6.26.0 `src/backend/cpu/CpuWorker.cpp` (merged via PR #3775) — note carefully which
buffer ends up where:

```cpp
                randomx_calculate_hash_next(m_vm, tempHash, m_job.blob(), job.size(), m_hash);

                if (RandomX_CurrentConfig.Tweak_V2_COMMITMENT) {
                    memcpy(m_commitment, m_hash, RANDOMX_HASH_SIZE);
                    randomx_calculate_commitment(prev_job, prev_job_size, m_hash, m_hash);
                    prev_job_size = job.size();
                    memcpy(prev_job, m_job.blob(), prev_job_size);
                }
```

then

```cpp
                    const uint64_t value = *reinterpret_cast<uint64_t*>(m_hash + (i * 32) + 24);
...
                    if (value < job.target()) {
                        uint8_t* extra_data = nullptr;

                        if (job.algorithm().family() == Algorithm::RANDOM_X) {
                            if (RandomX_CurrentConfig.Tweak_V2_COMMITMENT) {
                                extra_data = m_commitment;
                            }
...
                        JobResults::submit(job, current_job_nonces[i], m_hash + (i * 32), extra_data);
```

and in `src/base/net/stratum/Client.cpp`:

```cpp
    if (result.commitment()) {
        params.AddMember("commitment", StringRef(commitment), allocator);
    }
```

Decoding this (the buffer names are misleading — trace the copies):

1. `m_hash` initially holds the **RandomX hash**.
2. `m_commitment` gets a copy of that **raw RandomX hash**.
3. `m_hash` is then **overwritten in place with the Blake2b commitment**.
4. The target comparison (`value`, bytes 24..31 little-endian) is done on `m_hash` — i.e. **the
   commitment is what is compared against the target**, confirming the plan.
5. The stratum submit's **`result` field carries the commitment** (3rd arg to
   `JobResults::submit` → `JobResult::result()` → `Cvt::toHex(data, 65, result.result(), 32)`),
   and the **new `"commitment"` JSON field carries the raw RandomX hash** (extra_data →
   `JobResult::commitment()`).

⚠️ **This is the opposite of PLAN_RANDOMX_V2.md §5**, which assumed `result` = raw hash and
`commitment` = blake2b. Per the only existing implementation (xmrig), for `rx/2` jobs:

```json
"params": { "id": ..., "job_id": ..., "nonce": "<8hex>",
            "result": "<commitment hex, target-compared>",
            "commitment": "<raw RandomX hash hex>" }
```

This makes operational sense (pools' existing "compare `result` to target" logic keeps working;
the extra field supplies the hash needed to re-derive the commitment), but there is **no pool-side
reference implementation yet** — see Open Items. Update the plan before implementing Phase 5.

6. **Pipelining detail that directly affects MinerTim:** because `calculate_hash_next` returns the
   hash of the *previous* input, xmrig keeps `prev_job` (the previous blob incl. its nonce) and
   computes the commitment over `prev_job`, not the current blob. Our
   `calculate_hash_pipelined(next_input)` has the same off-by-one: the commitment for a returned
   hash must use the blob that *produced that hash*. Our miner loop already tracks
   `job_blob_current` for share submission, so this is a one-line pairing concern, but it must be
   the nonced blob, byte-for-byte, full length (`job.size()`, i.e. all 76 bytes of the hashing
   blob — not 39, not the 168-hex template).

Also from PR #3775 (SChernykh): *"`BlockTemplate.h/cpp` and `DaemonClient.h/cpp` were not updated
because Monero codebase hasn't implemented commitments yet, so there is no reference code."*

---

## 6. Everything verified UNCHANGED (v1.2.1 → master byte-diffs)

| Component | File(s) | Diff result |
|---|---|---|
| Dataset item computation (superscalarhash application, cache→dataset expansion) | `src/dataset.cpp` | **identical** |
| SuperscalarHash program generation | `src/superscalar.cpp` | **identical** |
| Blake2Generator | `src/blake2_generator.cpp` | **identical** |
| Instruction encoding/decoding (`Instruction`, mod bits, imm32) | `src/instruction.hpp` | **identical** |
| Blake2b | `src/blake2/*` | untouched by #317 (not in the PR file list) |
| Argon2d cache init (memory 256 MiB, 3 iters, 1 lane, salt `"RandomX\x03"`) | `configuration.h`, `argon2_*.c` | constants identical; argon files not touched |
| `AesGenerator1R` (scratchpad fill), `AesGenerator4R` (program gen), `AesHash1R` (final scratchpad fingerprint), incl. all four hard-coded key sets and initial states | `src/aes_hash.cpp` | only change: RISC-V vector dispatch (`#ifdef __riscv` blocks) + a typo fix ("intial"→"initial"); the x86/generic algorithm is line-identical |
| Soft AES round tables/logic | `src/soft_aes.cpp/.h` | tables renamed & exported (`randomx_aes_lut_enc[4][256]`), values identical |
| Entropy layout / `initialize()` (a-regs, `ma`, `mx`, readReg selection, `datasetOffset`, `eMask`) | `src/virtual_machine.cpp` | only change: `#ifndef __riscv` around the hard-AES probe |
| Outer hash driver (blake2b(input) → initScratchpad → 8 chained `run()`s with blake2b(RegisterFile) between → `hashAes1Rx4` + final blake2b) | `randomx_calculate_hash` in `src/randomx.cpp` | **identical** logic (quoted in full during research; no v2 branch) |
| Scratchpad reads/writes in loop steps 1–3, 9, 11–13; FP conversion rules; eMask; CBRANCH; all other instructions | `vm_interpreted.cpp`, `bytecode_machine.hpp` | no changes beyond §§2–4 above |
| Rounding-mode reset per hash (`resetRoundingMode`) | `virtual_machine.cpp` | identical |

Non-consensus API additions in master you can ignore: `randomx_get_cache_memory()`, and
`randomx_init_dataset` batching items in multiples of 4 (a perf refactor for the AVX2 dataset
init; item *values* are unchanged).

One real but non-consensus caveat: `executeBytecode`/`compileProgram` now take `randomx_flags`,
and per-instruction execution receives `flags` (the `RANDOMX_EXE_ARGS` macro grew a
`randomx_flags flags` parameter) — plumbing only, CFROUND is the sole consumer.

---

## 7. Test vectors

All from master `src/tests/tests.cpp` (key/input are ASCII, no trailing NUL; `calcStringHash`
passes `H - 1`). **All v2 hash vectors below run in light mode** (`randomx_create_vm(...,
cache, nullptr)`) — light and full mode produce identical hashes, so MinerTim can check them
against its full-dataset path too; for CI speed, cache-only (light) is enough: cache init ~1 s,
then 8 programs × 2048 iterations per hash (light-mode dataset items computed on the fly —
still well under a minute per hash even interpreted).

### 7.1 Full-hash vectors (key = `"test key 000"` unless noted)

| Test | Input | v1 hash (regression guard) | **v2 hash** |
|---|---|---|---|
| a | `"This is a test"` | `639183aae1bf4c9a35884cb46b09cad9175f04efd7684e7262a0ac1c2f0b4e3f` | `22ec6b861b3eb23686b2efbad69513c967ecfce80983df66c9c5b4fbfb4cdb6f` |
| b | `"Lorem ipsum dolor sit amet"` | `300a0adb47603dedb42228ccb2b211104f4da45af709cd7547cd049e9489c969` | `9e2c772c12fd48f93c14c97fdc89d556264d9100597023f44d9163e279012ecf` |
| c | `"sed do eiusmod tempor incididunt ut labore et dolore magna aliqua"` | `c36d4ed4191e617309867ed66a443be4075014e2b061bcdaf9ce7b721d2b77a8` | `4d6b063a1a603751d525f18a171336a4002f2f06df6c17e4b25fe17e17796e42` |
| d (key `"test key 001"`) | same as c | `e9ff4503201c0c2cca26d285c93ae883f9b1d30c9eb240b820756f2d5a7905fc` | `97024134686ce27d362ea8d86d8ef16483ac272abdabd46ef13359400777fe5e` |
| e (key `"test key 001"`) | hex blob `0b0b98bea7e805e0010a2126d287a2a0cc833d312cb786385a7c2f9de69d25537f584a9bc9977b00000000666fd8753bf61a8631f12984e3fd44f4014eca629276817b56f32e9b68bd82f416` (decoded to 76 bytes) | `c56414121acda1713c2f2a819d8ae38aed7c80c35c2a769298d34f03833cd5f1` | `c8e92c5f7c1946fecf06bc382b92e3111da38ee3e6a5ad90704e1a9d8aaf6e76` |

Verbatim source for (a):

```cpp
	auto test_a = [&] {
		alignas(16) char hash[RANDOMX_HASH_SIZE];
		calcStringHash("test key 000", "This is a test", &hash);
		assert(equalsHex(hash, (vm->getFlags() & RANDOMX_FLAG_V2) ? "22ec6b861b3eb23686b2efbad69513c967ecfce80983df66c9c5b4fbfb4cdb6f" : "639183aae1bf4c9a35884cb46b09cad9175f04efd7684e7262a0ac1c2f0b4e3f"));
	};
```

Test (e) is the Monero-shaped 76-byte blob — the most representative single vector for the miner
path. **Recommended minimal Rust test set: v2 (a) + v2 (e) + the two commitment vectors below.**
The v1↔v2 switch test (`test_switch`, tests.cpp ~line 1192) additionally proves the same VM can
flip `RANDOMX_FLAG_V2` per hash without cache re-init — dataset/cache contents are
version-independent, so **MinerTim's shared dataset needs no v2 rebuild** (same seed → same
dataset for rx/0 and rx/2).

### 7.2 Commitment vectors

Master (runs on a **v2** VM — hash inside is v2 test-a's hash):

```cpp
	runTest("Commitment test", stringsEqual(RANDOMX_ARGON_SALT, "RandomX\x03"), []() {
		alignas(16) char hash[RANDOMX_HASH_SIZE];
		calcStringCommitment("test key 000", "This is a test", &hash);
		assert(equalsHex(hash, "133be717399046b03ae82ce8ddd9d1ee4d3ea7fca03a50dec09b6848cbb98e18"));
	});
```

where `calcStringCommitment` = `randomx_calculate_hash(vm, input, H-1, output)` then
`randomx_calculate_commitment(input, H - 1, output, output)`.

This decomposes into two pure-Blake2b vectors that need **no VM at all** (given the §7.1 hashes):

- **v2-based:** `blake2b_256("This is a test" ‖ hex"22ec6b86…cdb6f")` =
  `133be717399046b03ae82ce8ddd9d1ee4d3ea7fca03a50dec09b6848cbb98e18`
- **v1-based** (from v1.2.1 tests.cpp, same input, v1 hash `639183aa…b4e3f`):
  `d53ccf348b75291b7be76f0a7ac8208bbced734b912f6fca60539ab6f86be919`
  (this is the vector already cited in PLAN_RANDOMX_V2.md — input bytes now pinned: the 14 ASCII
  bytes `This is a test` followed by the 32-byte v1 hash)

Both run in microseconds — ideal first test for Phase 3.

### 7.3 xmrig chained benchmark checkpoints (optional, heavier)

xmrig's `BenchState_test.h` (PR #3769) has RX_V2 checkpoints for its 1-thread chained benchmark
(each hash's blob seeded from the previous; algo `rx/2`, benchmark key/blob per xmrig's benchmark
spec): `10000 → 0x90eb7c07cd9e0d90`, `250000 → 0xf83b6d9d355ee5b1`, … `10000000 →
0x7efbddff3f30fb74` (hashCheck1T table). Only worth wiring if we ever adopt xmrig's benchmark
blob format; the §7.1/7.2 vectors are sufficient for correctness.

---

## 8. Miner integration facts (xmrig v6.26.0)

- Algorithm id: `src/base/crypto/Algorithm.h`:
  `RX_V2 = 0x72151202,   // "rx/2"             RandomX (Monero v2).`
  Name `"rx/2"`, aliases `"randomx/v2"`, `"rx/v2"` (`Algorithm.cpp`). `RxAlgo.cpp` maps
  `Algorithm::RX_V2 → &RandomX_MoneroConfigV2` (the config quoted in §0).
- Selection is by the **job's `algo` field from the pool**, not by blob major-version sniffing;
  xmrig advertises its supported-algo list in the stratum login and the pool tags each job
  `"algo":"rx/2"`. (MinerTim currently hard-sends `"algo":"rx/0"` in login — for v2 we must at
  minimum honour a per-job `algo` and pick the VM version from it; blob byte 0 ≥ 17 is a
  cross-check, not the trigger xmrig uses.)
- Wire format for submits: see §5.2 (result = commitment, `commitment` = raw hash).
- The seed-hash / dataset lifecycle is unchanged; rx/0 and rx/2 share dataset contents for the
  same seed (see §7.1 note on `test_switch`).

MinerTim phase mapping stays as in PLAN_RANDOMX_V2.md with three corrections:
1. **Add Phase 4b: prefetch/`mp` change** (§4) — the plan's change-list missed it; it's 2 lines in
   `vm.rs`'s loop (XOR into `mem_ma`, prefetch after XOR before swap) plus the JIT/pipeline
   equivalents if we ever emit the loop tail in JIT.
2. **Fix Phase 5 field semantics** (§5.2): `result` = commitment, new field = raw hash;
   `meets_target` takes the commitment.
3. Phase 2's "key derivation TBD" is resolved: keys are the live e-registers, §3.

---

## 9. Open items

| # | Item | Where the answer will appear |
|---|---|---|
| 1 | **Monero HF v17 activation height** — still absent from `mainnet_hard_forks` as of 2026-08-15. The blocker for shipping. | https://github.com/monero-project/monero/blob/master/src/hardforks/hardforks.cpp (watch for a `{ 17, ... }` entry); monero-project/monero#10038 |
| 2 | **Pool-side confirmation of the `result`/`commitment` field semantics.** Only xmrig's miner side exists; SChernykh explicitly noted no Monero/pool reference code yet (PR #3775). p2pool master has no commitment handling yet. Risk: a pool could interpret the fields differently until a reference lands. | https://github.com/SChernykh/p2pool (stratum server), monero-project/monero#8827, xmrig-proxy |
| 3 | **Login `algo` negotiation for rx/2 on real pools** — presumably list-based like xmrig (`"algo": ["rx/0","rx/2",...]` in login), but no pool implements rx/2 jobs yet to verify against. | pool operator docs / p2pool stratum once implemented |
| 4 | Monero-side consensus glue (`HF_VERSION_RANDOMX_V2`, whether the *block hash* stored on-chain is the hash or the commitment) — not yet in monero master. Affects nothing in the miner loop besides §5. | monero-project/monero#10038 / #8827 |
| 5 | tevador may cut a tagged release (v2.0?) after #317; this document quotes master @ 2026-08-15. Re-diff against the tag before implementation starts. | https://github.com/tevador/RandomX/releases |

Everything in §§1–7 is pinned to merged code and needs no further research.
