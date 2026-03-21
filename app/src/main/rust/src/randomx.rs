/// Simplified RandomX hash implementation for mobile mining.
/// Implements Blake2b, SipHash-2-4, and a simplified RandomX VM
/// with program generation and execution on a scratchpad.

const BLAKE2B_BLOCKBYTES: usize = 128;
#[allow(dead_code)]
const BLAKE2B_OUTBYTES: usize = 64;

const BLAKE2B_IV: [u64; 8] = [
    0x6a09e667f3bcc908,
    0xbb67ae8584caa73b,
    0x3c6ef372fe94f82b,
    0xa54ff53a5f1d36f6,
    0x510e527fade682d1,
    0x9b05688c2b3e6c1f,
    0x1f83d9abfb41bd6b,
    0x5be0cd19137e2179,
];

const BLAKE2B_SIGMA: [[usize; 16]; 12] = [
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

// RandomX constants
const RANDOMX_SCRATCHPAD_L3: usize = 2 * 1024 * 1024; // 2 MB
const RANDOMX_SCRATCHPAD_L2: usize = 256 * 1024; // 256 KB
const RANDOMX_SCRATCHPAD_L1: usize = 16 * 1024; // 16 KB
const RANDOMX_PROGRAM_SIZE: usize = 256;
const RANDOMX_NUM_REGISTERS: usize = 8;

// --- Blake2b ---

struct Blake2bState {
    h: [u64; 8],
    t: [u64; 2],
    buf: [u8; BLAKE2B_BLOCKBYTES],
    buf_len: usize,
}

impl Blake2bState {
    fn new(out_len: usize) -> Self {
        let mut h = BLAKE2B_IV;
        h[0] ^= 0x01010000 ^ (out_len as u64);
        Self {
            h,
            t: [0; 2],
            buf: [0u8; BLAKE2B_BLOCKBYTES],
            buf_len: 0,
        }
    }

    fn update(&mut self, data: &[u8]) {
        let mut offset = 0;
        let len = data.len();

        if self.buf_len + len > BLAKE2B_BLOCKBYTES {
            // Fill buffer and compress
            let fill = BLAKE2B_BLOCKBYTES - self.buf_len;
            if fill <= len {
                self.buf[self.buf_len..BLAKE2B_BLOCKBYTES].copy_from_slice(&data[..fill]);
                self.t[0] = self.t[0].wrapping_add(BLAKE2B_BLOCKBYTES as u64);
                if self.t[0] < BLAKE2B_BLOCKBYTES as u64 {
                    self.t[1] = self.t[1].wrapping_add(1);
                }
                self.compress(false);
                self.buf_len = 0;
                offset = fill;
            }

            // Compress full blocks
            while offset + BLAKE2B_BLOCKBYTES < len {
                self.buf.copy_from_slice(&data[offset..offset + BLAKE2B_BLOCKBYTES]);
                self.t[0] = self.t[0].wrapping_add(BLAKE2B_BLOCKBYTES as u64);
                if self.t[0] < BLAKE2B_BLOCKBYTES as u64 {
                    self.t[1] = self.t[1].wrapping_add(1);
                }
                self.compress(false);
                offset += BLAKE2B_BLOCKBYTES;
            }
        }

        // Buffer remaining
        let remaining = len - offset;
        if remaining > 0 {
            self.buf[self.buf_len..self.buf_len + remaining].copy_from_slice(&data[offset..]);
            self.buf_len += remaining;
        }
    }

    fn finalize(mut self, out: &mut [u8]) {
        self.t[0] = self.t[0].wrapping_add(self.buf_len as u64);
        if self.t[0] < self.buf_len as u64 {
            self.t[1] = self.t[1].wrapping_add(1);
        }

        // Pad remaining buffer with zeros
        for i in self.buf_len..BLAKE2B_BLOCKBYTES {
            self.buf[i] = 0;
        }

        self.compress(true);

        // Write output
        let mut pos = 0;
        for &word in &self.h {
            let bytes = word.to_le_bytes();
            for &b in &bytes {
                if pos < out.len() {
                    out[pos] = b;
                    pos += 1;
                }
            }
        }
    }

    fn compress(&mut self, last: bool) {
        let mut v = [0u64; 16];
        v[..8].copy_from_slice(&self.h);
        v[8..16].copy_from_slice(&BLAKE2B_IV);

        v[12] ^= self.t[0];
        v[13] ^= self.t[1];

        if last {
            v[14] = !v[14];
        }

        let mut m = [0u64; 16];
        for i in 0..16 {
            let offset = i * 8;
            if offset + 8 <= BLAKE2B_BLOCKBYTES {
                m[i] = u64::from_le_bytes([
                    self.buf[offset],
                    self.buf[offset + 1],
                    self.buf[offset + 2],
                    self.buf[offset + 3],
                    self.buf[offset + 4],
                    self.buf[offset + 5],
                    self.buf[offset + 6],
                    self.buf[offset + 7],
                ]);
            }
        }

        for i in 0..12 {
            let s = &BLAKE2B_SIGMA[i];
            blake2b_g(&mut v, 0, 4, 8, 12, m[s[0]], m[s[1]]);
            blake2b_g(&mut v, 1, 5, 9, 13, m[s[2]], m[s[3]]);
            blake2b_g(&mut v, 2, 6, 10, 14, m[s[4]], m[s[5]]);
            blake2b_g(&mut v, 3, 7, 11, 15, m[s[6]], m[s[7]]);
            blake2b_g(&mut v, 0, 5, 10, 15, m[s[8]], m[s[9]]);
            blake2b_g(&mut v, 1, 6, 11, 12, m[s[10]], m[s[11]]);
            blake2b_g(&mut v, 2, 7, 8, 13, m[s[12]], m[s[13]]);
            blake2b_g(&mut v, 3, 4, 9, 14, m[s[14]], m[s[15]]);
        }

        for i in 0..8 {
            self.h[i] ^= v[i] ^ v[i + 8];
        }
    }
}

#[inline]
fn blake2b_g(v: &mut [u64; 16], a: usize, b: usize, c: usize, d: usize, x: u64, y: u64) {
    v[a] = v[a].wrapping_add(v[b]).wrapping_add(x);
    v[d] = (v[d] ^ v[a]).rotate_right(32);
    v[c] = v[c].wrapping_add(v[d]);
    v[b] = (v[b] ^ v[c]).rotate_right(24);
    v[a] = v[a].wrapping_add(v[b]).wrapping_add(y);
    v[d] = (v[d] ^ v[a]).rotate_right(16);
    v[c] = v[c].wrapping_add(v[d]);
    v[b] = (v[b] ^ v[c]).rotate_right(63);
}

fn blake2b(input: &[u8], out_len: usize) -> Vec<u8> {
    let mut state = Blake2bState::new(out_len);
    state.update(input);
    let mut out = vec![0u8; out_len];
    state.finalize(&mut out);
    out
}

fn blake2b_256(input: &[u8]) -> [u8; 32] {
    let result = blake2b(input, 32);
    let mut out = [0u8; 32];
    out.copy_from_slice(&result);
    out
}

fn blake2b_512(input: &[u8]) -> [u8; 64] {
    let result = blake2b(input, 64);
    let mut out = [0u8; 64];
    out.copy_from_slice(&result);
    out
}

// --- SipHash-2-4 ---

pub fn siphash24(key: &[u8; 16], data: &[u8]) -> u64 {
    let k0 = u64::from_le_bytes([key[0], key[1], key[2], key[3], key[4], key[5], key[6], key[7]]);
    let k1 = u64::from_le_bytes([
        key[8], key[9], key[10], key[11], key[12], key[13], key[14], key[15],
    ]);

    let mut v0: u64 = k0 ^ 0x736f6d6570736575;
    let mut v1: u64 = k1 ^ 0x646f72616e646f6d;
    let mut v2: u64 = k0 ^ 0x6c7967656e657261;
    let mut v3: u64 = k1 ^ 0x7465646279746573;

    let blocks = data.len() / 8;
    for i in 0..blocks {
        let offset = i * 8;
        let m = u64::from_le_bytes([
            data[offset],
            data[offset + 1],
            data[offset + 2],
            data[offset + 3],
            data[offset + 4],
            data[offset + 5],
            data[offset + 6],
            data[offset + 7],
        ]);

        v3 ^= m;
        sipround(&mut v0, &mut v1, &mut v2, &mut v3);
        sipround(&mut v0, &mut v1, &mut v2, &mut v3);
        v0 ^= m;
    }

    // Last block with length byte
    let remaining = data.len() % 8;
    let offset = blocks * 8;
    let mut last: u64 = (data.len() as u64) << 56;
    for i in 0..remaining {
        last |= (data[offset + i] as u64) << (i * 8);
    }

    v3 ^= last;
    sipround(&mut v0, &mut v1, &mut v2, &mut v3);
    sipround(&mut v0, &mut v1, &mut v2, &mut v3);
    v0 ^= last;

    // Finalization
    v2 ^= 0xff;
    sipround(&mut v0, &mut v1, &mut v2, &mut v3);
    sipround(&mut v0, &mut v1, &mut v2, &mut v3);
    sipround(&mut v0, &mut v1, &mut v2, &mut v3);
    sipround(&mut v0, &mut v1, &mut v2, &mut v3);

    v0 ^ v1 ^ v2 ^ v3
}

#[inline]
fn sipround(v0: &mut u64, v1: &mut u64, v2: &mut u64, v3: &mut u64) {
    *v0 = v0.wrapping_add(*v1);
    *v1 = v1.rotate_left(13);
    *v1 ^= *v0;
    *v0 = v0.rotate_left(32);
    *v2 = v2.wrapping_add(*v3);
    *v3 = v3.rotate_left(16);
    *v3 ^= *v2;
    *v0 = v0.wrapping_add(*v3);
    *v3 = v3.rotate_left(21);
    *v3 ^= *v0;
    *v2 = v2.wrapping_add(*v1);
    *v1 = v1.rotate_left(17);
    *v1 ^= *v2;
    *v2 = v2.rotate_left(32);
}

// --- RandomX VM ---

#[derive(Clone, Copy, Debug, PartialEq)]
#[repr(u8)]
enum Opcode {
    IaddRs = 0,
    IsubR = 1,
    ImulR = 2,
    ImulhR = 3,
    IsmulhR = 4,
    ImulRcp = 5,
    InegR = 6,
    IxorR = 7,
    IrorR = 8,
    IrolR = 9,
    IswapR = 10,
    FswapR = 11,
    FaddR = 12,
    FaddM = 13,
    FsubR = 14,
    FsubM = 15,
    FscalR = 16,
    FmulR = 17,
    FdivM = 18,
    FsqrtR = 19,
    Cbranch = 20,
    Cfround = 21,
    Istore = 22,
    Nop = 23,
    IaddM = 24,
    IsubM = 25,
    IxorM = 26,
}

impl Opcode {
    fn from_byte(b: u8) -> Self {
        match b % 27 {
            0 => Opcode::IaddRs,
            1 => Opcode::IsubR,
            2 => Opcode::ImulR,
            3 => Opcode::ImulhR,
            4 => Opcode::IsmulhR,
            5 => Opcode::ImulRcp,
            6 => Opcode::InegR,
            7 => Opcode::IxorR,
            8 => Opcode::IrorR,
            9 => Opcode::IrolR,
            10 => Opcode::IswapR,
            11 => Opcode::FswapR,
            12 => Opcode::FaddR,
            13 => Opcode::FaddM,
            14 => Opcode::FsubR,
            15 => Opcode::FsubM,
            16 => Opcode::FscalR,
            17 => Opcode::FmulR,
            18 => Opcode::FdivM,
            19 => Opcode::FsqrtR,
            20 => Opcode::Cbranch,
            21 => Opcode::Cfround,
            22 => Opcode::Istore,
            23 => Opcode::Nop,
            24 => Opcode::IaddM,
            25 => Opcode::IsubM,
            26 => Opcode::IxorM,
            _ => Opcode::Nop,
        }
    }
}

#[derive(Clone, Copy)]
struct Instruction {
    opcode: Opcode,
    dst: usize,
    src: usize,
    imm32: u32,
    shift: u8,
}

struct Program {
    instructions: Vec<Instruction>,
}

/// Xorshift64 PRNG for program generation
struct Xorshift64 {
    state: u64,
}

impl Xorshift64 {
    fn new(seed: u64) -> Self {
        Self {
            state: if seed == 0 { 1 } else { seed },
        }
    }

    fn next(&mut self) -> u64 {
        let mut x = self.state;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.state = x;
        x
    }
}

pub struct RandomXVM {
    scratchpad: Vec<u8>,
    cache: Vec<u64>,
    registers: [u64; RANDOMX_NUM_REGISTERS],
    float_registers: [f64; RANDOMX_NUM_REGISTERS],
    program: Option<Program>,
    seed_hash: [u8; 32],
}

impl RandomXVM {
    pub fn new(key: &[u8]) -> Self {
        let seed_hash = blake2b_256(key);

        let mut vm = Self {
            scratchpad: vec![0u8; RANDOMX_SCRATCHPAD_L3],
            cache: Vec::new(),
            registers: [0u64; RANDOMX_NUM_REGISTERS],
            float_registers: [0.0f64; RANDOMX_NUM_REGISTERS],
            program: None,
            seed_hash,
        };

        vm.init_cache(key);
        vm.init_scratchpad();
        vm
    }

    fn init_cache(&mut self, key: &[u8]) {
        // Generate cache from key using Blake2b in Argon2-like fashion
        let cache_size = 256 * 1024; // 256 KB in u64s = 32768 entries
        let num_entries = cache_size / 8;
        self.cache = vec![0u64; num_entries];

        // Initial fill with Blake2b hashes
        let mut hash_input = blake2b_512(key);
        for i in 0..num_entries {
            let idx = i % 8;
            if idx == 0 && i > 0 {
                hash_input = blake2b_512(&hash_input);
            }
            self.cache[i] = u64::from_le_bytes([
                hash_input[idx * 8],
                hash_input[idx * 8 + 1],
                hash_input[idx * 8 + 2],
                hash_input[idx * 8 + 3],
                hash_input[idx * 8 + 4],
                hash_input[idx * 8 + 5],
                hash_input[idx * 8 + 6],
                hash_input[idx * 8 + 7],
            ]);
        }

        // Mix cache entries using SipHash
        let sip_key: [u8; 16] = {
            let h = blake2b(key, 16);
            let mut k = [0u8; 16];
            k.copy_from_slice(&h);
            k
        };

        for round in 0..4u32 {
            for i in 0..num_entries {
                let prev = if i == 0 {
                    self.cache[num_entries - 1]
                } else {
                    self.cache[i - 1]
                };
                let addr = (prev as usize ^ (round as usize * 137 + i)) % num_entries;
                let cache_val = self.cache[addr];
                let input_bytes = cache_val.to_le_bytes();
                let hash = siphash24(&sip_key, &input_bytes);
                self.cache[i] ^= hash;
            }
        }
    }

    fn init_scratchpad(&mut self) {
        // Fill scratchpad from cache
        let cache_len = self.cache.len();
        if cache_len == 0 {
            return;
        }

        let mut idx = 0usize;
        let scratchpad_len = self.scratchpad.len();

        while idx + 8 <= scratchpad_len {
            let cache_idx = (idx / 8) % cache_len;
            let val = self.cache[cache_idx];
            let bytes = val.to_le_bytes();
            self.scratchpad[idx..idx + 8].copy_from_slice(&bytes);
            idx += 8;
        }
    }

    fn generate_program(&mut self, seed: &[u8]) -> Program {
        let hash = blake2b_512(seed);
        let seed_val = u64::from_le_bytes([
            hash[0], hash[1], hash[2], hash[3], hash[4], hash[5], hash[6], hash[7],
        ]);

        let mut rng = Xorshift64::new(seed_val);
        let mut instructions = Vec::with_capacity(RANDOMX_PROGRAM_SIZE);

        for _ in 0..RANDOMX_PROGRAM_SIZE {
            let r = rng.next();
            let opcode = Opcode::from_byte((r & 0xFF) as u8);
            let dst = ((r >> 8) & 0x07) as usize;
            let src = ((r >> 11) & 0x07) as usize;
            let imm32 = ((r >> 16) & 0xFFFFFFFF) as u32;
            let shift = ((r >> 48) & 0x03) as u8;

            instructions.push(Instruction {
                opcode,
                dst,
                src,
                imm32,
                shift,
            });
        }

        Program { instructions }
    }

    fn execute_program(&mut self) {
        let program = match &self.program {
            Some(p) => p,
            None => return,
        };

        let scratchpad_mask_l3 = (RANDOMX_SCRATCHPAD_L3 - 8) as u64;
        let scratchpad_mask_l2 = (RANDOMX_SCRATCHPAD_L2 - 8) as u64;
        let scratchpad_mask_l1 = (RANDOMX_SCRATCHPAD_L1 - 8) as u64;

        // Clone instructions to avoid borrow issues
        let instructions: Vec<Instruction> = program.instructions.clone();

        for inst in &instructions {
            match inst.opcode {
                Opcode::IaddRs => {
                    let src_val = self.registers[inst.src] << inst.shift;
                    self.registers[inst.dst] = self.registers[inst.dst].wrapping_add(src_val);
                }
                Opcode::IsubR => {
                    self.registers[inst.dst] =
                        self.registers[inst.dst].wrapping_sub(self.registers[inst.src]);
                }
                Opcode::ImulR => {
                    self.registers[inst.dst] =
                        self.registers[inst.dst].wrapping_mul(self.registers[inst.src]);
                }
                Opcode::ImulhR => {
                    let a = self.registers[inst.dst] as u128;
                    let b = self.registers[inst.src] as u128;
                    self.registers[inst.dst] = ((a * b) >> 64) as u64;
                }
                Opcode::IsmulhR => {
                    let a = self.registers[inst.dst] as i64 as i128;
                    let b = self.registers[inst.src] as i64 as i128;
                    self.registers[inst.dst] = ((a * b) >> 64) as u64;
                }
                Opcode::ImulRcp => {
                    if inst.imm32 != 0 {
                        let rcp = u64::MAX / (inst.imm32 as u64);
                        self.registers[inst.dst] = self.registers[inst.dst].wrapping_mul(rcp);
                    }
                }
                Opcode::InegR => {
                    self.registers[inst.dst] = (self.registers[inst.dst] as i64).wrapping_neg() as u64;
                }
                Opcode::IxorR => {
                    self.registers[inst.dst] ^= self.registers[inst.src];
                }
                Opcode::IrorR => {
                    let shift = (self.registers[inst.src] & 63) as u32;
                    self.registers[inst.dst] = self.registers[inst.dst].rotate_right(shift);
                }
                Opcode::IrolR => {
                    let shift = (self.registers[inst.src] & 63) as u32;
                    self.registers[inst.dst] = self.registers[inst.dst].rotate_left(shift);
                }
                Opcode::IswapR => {
                    if inst.dst != inst.src {
                        let tmp = self.registers[inst.dst];
                        self.registers[inst.dst] = self.registers[inst.src];
                        self.registers[inst.src] = tmp;
                    }
                }
                Opcode::FswapR => {
                    // Swap high and low halves of a float register
                    let val = self.float_registers[inst.dst];
                    let bits = val.to_bits();
                    let swapped = (bits >> 32) | (bits << 32);
                    self.float_registers[inst.dst] = f64::from_bits(swapped);
                }
                Opcode::FaddR => {
                    self.float_registers[inst.dst] += self.float_registers[inst.src];
                }
                Opcode::FaddM => {
                    let addr = (self.registers[inst.src].wrapping_add(inst.imm32 as u64))
                        & scratchpad_mask_l2;
                    let val = self.read_scratchpad_f64(addr as usize);
                    self.float_registers[inst.dst] += val;
                }
                Opcode::FsubR => {
                    self.float_registers[inst.dst] -= self.float_registers[inst.src];
                }
                Opcode::FsubM => {
                    let addr = (self.registers[inst.src].wrapping_add(inst.imm32 as u64))
                        & scratchpad_mask_l1;
                    let val = self.read_scratchpad_f64(addr as usize);
                    self.float_registers[inst.dst] -= val;
                }
                Opcode::FscalR => {
                    let bits = self.float_registers[inst.dst].to_bits();
                    // Toggle the exponent's MSB
                    self.float_registers[inst.dst] = f64::from_bits(bits ^ (1u64 << 63));
                }
                Opcode::FmulR => {
                    self.float_registers[inst.dst] *= self.float_registers[inst.src];
                    if !self.float_registers[inst.dst].is_finite() {
                        self.float_registers[inst.dst] = 0.0;
                    }
                }
                Opcode::FdivM => {
                    let addr = (self.registers[inst.src].wrapping_add(inst.imm32 as u64))
                        & scratchpad_mask_l1;
                    let val = self.read_scratchpad_f64(addr as usize);
                    if val != 0.0 && val.is_finite() {
                        self.float_registers[inst.dst] /= val;
                    }
                    if !self.float_registers[inst.dst].is_finite() {
                        self.float_registers[inst.dst] = 0.0;
                    }
                }
                Opcode::FsqrtR => {
                    let val = self.float_registers[inst.dst].abs();
                    self.float_registers[inst.dst] = val.sqrt();
                }
                Opcode::Cbranch => {
                    // Conditional branch: modify register and continue (simplified)
                    self.registers[inst.dst] =
                        self.registers[inst.dst].wrapping_add(inst.imm32 as u64);
                }
                Opcode::Cfround => {
                    // Change float rounding mode (simplified: no-op on software implementation)
                }
                Opcode::Istore => {
                    let addr = (self.registers[inst.dst].wrapping_add(inst.imm32 as u64))
                        & scratchpad_mask_l3;
                    let val = self.registers[inst.src];
                    self.write_scratchpad_u64(addr as usize, val);
                }
                Opcode::Nop => {}
                Opcode::IaddM => {
                    let addr = (self.registers[inst.src].wrapping_add(inst.imm32 as u64))
                        & scratchpad_mask_l2;
                    let val = self.read_scratchpad_u64(addr as usize);
                    self.registers[inst.dst] = self.registers[inst.dst].wrapping_add(val);
                }
                Opcode::IsubM => {
                    let addr = (self.registers[inst.src].wrapping_add(inst.imm32 as u64))
                        & scratchpad_mask_l2;
                    let val = self.read_scratchpad_u64(addr as usize);
                    self.registers[inst.dst] = self.registers[inst.dst].wrapping_sub(val);
                }
                Opcode::IxorM => {
                    let addr = (self.registers[inst.src].wrapping_add(inst.imm32 as u64))
                        & scratchpad_mask_l3;
                    let val = self.read_scratchpad_u64(addr as usize);
                    self.registers[inst.dst] ^= val;
                }
            }
        }
    }

    fn read_scratchpad_u64(&self, addr: usize) -> u64 {
        if addr + 8 <= self.scratchpad.len() {
            u64::from_le_bytes([
                self.scratchpad[addr],
                self.scratchpad[addr + 1],
                self.scratchpad[addr + 2],
                self.scratchpad[addr + 3],
                self.scratchpad[addr + 4],
                self.scratchpad[addr + 5],
                self.scratchpad[addr + 6],
                self.scratchpad[addr + 7],
            ])
        } else {
            0
        }
    }

    fn read_scratchpad_f64(&self, addr: usize) -> f64 {
        let bits = self.read_scratchpad_u64(addr);
        let val = f64::from_bits(bits);
        if val.is_finite() {
            val
        } else {
            0.0
        }
    }

    fn write_scratchpad_u64(&mut self, addr: usize, val: u64) {
        if addr + 8 <= self.scratchpad.len() {
            let bytes = val.to_le_bytes();
            self.scratchpad[addr..addr + 8].copy_from_slice(&bytes);
        }
    }

    pub fn calculate_hash(&mut self, input: &[u8]) -> [u8; 32] {
        // 1. Hash the input to get program seed
        let input_hash = blake2b_512(input);

        // 2. Initialize registers from input hash
        for i in 0..RANDOMX_NUM_REGISTERS {
            let offset = i * 8;
            self.registers[i] = u64::from_le_bytes([
                input_hash[offset],
                input_hash[offset + 1],
                input_hash[offset + 2],
                input_hash[offset + 3],
                input_hash[offset + 4],
                input_hash[offset + 5],
                input_hash[offset + 6],
                input_hash[offset + 7],
            ]);
            // Initialize float registers from second half of hash or wrap
            let f_offset = (i * 8 + 32) % 64;
            let float_bits = u64::from_le_bytes([
                input_hash[f_offset],
                input_hash[f_offset + 1],
                input_hash[f_offset + 2],
                input_hash[f_offset + 3],
                input_hash[f_offset + 4],
                input_hash[f_offset + 5],
                input_hash[f_offset + 6],
                input_hash[f_offset + 7],
            ]);
            // Ensure a valid finite float
            let float_val = f64::from_bits(float_bits);
            self.float_registers[i] = if float_val.is_finite() {
                float_val
            } else {
                (i as f64) + 1.0
            };
        }

        // 3. Read scratchpad data into registers to mix state
        for i in 0..RANDOMX_NUM_REGISTERS {
            let addr = (self.registers[i] as usize) % (RANDOMX_SCRATCHPAD_L3 - 8);
            let sp_val = self.read_scratchpad_u64(addr);
            self.registers[i] ^= sp_val;
        }

        // 4. Generate and execute program (8 rounds, like real RandomX)
        let mut prog_seed = input_hash.to_vec();
        for _round in 0..8 {
            let program = self.generate_program(&prog_seed);
            self.program = Some(program);
            self.execute_program();

            // Update program seed from registers for next round
            let mut new_seed = Vec::with_capacity(64);
            for &r in &self.registers {
                new_seed.extend_from_slice(&r.to_le_bytes());
            }
            prog_seed = new_seed;
        }

        // 5. Write registers back to scratchpad
        for i in 0..RANDOMX_NUM_REGISTERS {
            let addr = (self.registers[i] as usize) % (RANDOMX_SCRATCHPAD_L3 - 8);
            self.write_scratchpad_u64(addr, self.registers[i]);
        }

        // 6. Final hash: combine registers and scratchpad samples via Blake2b
        let mut final_input = Vec::with_capacity(256);
        for &r in &self.registers {
            final_input.extend_from_slice(&r.to_le_bytes());
        }
        for &f in &self.float_registers {
            final_input.extend_from_slice(&f.to_bits().to_le_bytes());
        }
        // Sample scratchpad at register-derived positions
        for i in 0..RANDOMX_NUM_REGISTERS {
            let addr = (self.registers[i] as usize) % (RANDOMX_SCRATCHPAD_L3 - 8);
            let sp_val = self.read_scratchpad_u64(addr);
            final_input.extend_from_slice(&sp_val.to_le_bytes());
        }

        blake2b_256(&final_input)
    }
}
