// Index-based loops in the RandomX modules deliberately mirror the C++
// reference implementation; rewriting them as iterators hurts auditability.
#![allow(clippy::needless_range_loop)]
#![allow(clippy::explicit_counter_loop)]

pub mod donate;
pub mod hex;
pub mod miner;
pub mod pool_connection;
pub mod randomx;
