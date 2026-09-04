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

// Gated on the architecture alone, deliberately: `vm.rs` carries ~40
// `#[cfg(target_arch = "aarch64")]` sites that reference this module, so
// narrowing the gate here would mean rewriting every one of them. The OS
// requirement is enforced one level down instead — `jit::memory` has a
// `compile_error!` for any OS other than macOS or Linux, which fails the build
// with a message that names the problem.
#[cfg(target_arch = "aarch64")]
pub mod jit;

#[cfg(test)]
mod tests;
