// Blake2Generator - PRNG based on Blake2b-512
// Reference: RandomX src/blake2_generator.cpp

pub struct Blake2Generator {
    data: [u8; 64],
    data_index: usize,
}

impl Blake2Generator {
    /// Create a new generator from seed and nonce.
    /// Seed is truncated to 60 bytes, nonce stored at bytes 60-63.
    pub fn new(seed: &[u8], nonce: u32) -> Self {
        let mut data = [0u8; 64];
        let copy_len = seed.len().min(60);
        data[..copy_len].copy_from_slice(&seed[..copy_len]);
        data[60..64].copy_from_slice(&nonce.to_le_bytes());
        Self {
            data,
            data_index: 64, // force initial hash on first use
        }
    }

    pub fn get_byte(&mut self) -> u8 {
        self.check_data(1);
        let b = self.data[self.data_index];
        self.data_index += 1;
        b
    }

    pub fn get_u32(&mut self) -> u32 {
        self.check_data(4);
        let val = u32::from_le_bytes([
            self.data[self.data_index],
            self.data[self.data_index + 1],
            self.data[self.data_index + 2],
            self.data[self.data_index + 3],
        ]);
        self.data_index += 4;
        val
    }

    fn check_data(&mut self, bytes_needed: usize) {
        if self.data_index + bytes_needed > 64 {
            self.data = super::blake2b::blake2b_512(&self.data);
            self.data_index = 0;
        }
    }
}
