// Pure Rust RandomX implementation (rx/0, light + full mode)
// Reference: https://github.com/tevador/RandomX

pub mod blake2b;
pub mod soft_aes;
pub mod aes_hash;
pub mod argon2d;
pub mod blake2gen;
pub mod superscalar;
pub mod dataset;
pub mod vm;

#[cfg(target_arch = "aarch64")]
pub mod jit;

#[cfg(test)]
mod tests;
