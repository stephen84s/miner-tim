// RandomX JIT compiler for aarch64
// Compiles BytecodeInstruction[256] to native ARM64 code.

pub mod memory;
pub mod aarch64;
pub mod compiler;

pub use compiler::JitCompiler;
