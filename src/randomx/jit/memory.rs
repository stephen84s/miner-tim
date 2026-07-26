// JIT memory management for aarch64 macOS
// Uses mmap with MAP_JIT, W^X toggle via pthread_jit_write_protect_np,
// and sys_icache_invalidate for cache coherency.

use std::ptr;

unsafe extern "C" {
    fn pthread_jit_write_protect_np(enabled: i32);
    fn sys_icache_invalidate(start: *mut u8, size: usize);
}

const MAP_ANON: i32 = 0x1000;
const MAP_PRIVATE: i32 = 0x0002;
const MAP_JIT: i32 = 0x0800;
const PROT_READ: i32 = 0x01;
const PROT_WRITE: i32 = 0x02;
const PROT_EXEC: i32 = 0x04;
const MAP_FAILED: *mut u8 = !0 as *mut u8;

unsafe extern "C" {
    fn mmap(addr: *mut u8, len: usize, prot: i32, flags: i32, fd: i32, offset: i64) -> *mut u8;
    fn munmap(addr: *mut u8, len: usize) -> i32;
}

/// Size of JIT code buffer per program (64KB — generous headroom over ~16KB typical)
pub const JIT_CODE_SIZE: usize = 64 * 1024;

pub struct JitMemory {
    ptr: *mut u8,
    size: usize,
    code_len: usize, // actual bytes written
}

unsafe impl Send for JitMemory {}

impl JitMemory {
    /// Allocate a new JIT memory region with MAP_JIT.
    pub fn new(size: usize) -> Result<Self, &'static str> {
        let ptr = unsafe {
            mmap(
                ptr::null_mut(),
                size,
                PROT_READ | PROT_WRITE | PROT_EXEC,
                MAP_ANON | MAP_PRIVATE | MAP_JIT,
                -1,
                0,
            )
        };
        if ptr == MAP_FAILED || ptr.is_null() {
            return Err("mmap MAP_JIT failed");
        }
        Ok(JitMemory {
            ptr,
            size,
            code_len: 0,
        })
    }

    /// Enable writing to JIT memory (disables execute permission).
    #[inline]
    pub fn enable_write(&self) {
        unsafe { pthread_jit_write_protect_np(0) };
    }

    /// Enable execute permission (disables write).
    /// Also flushes the instruction cache.
    #[inline]
    pub fn enable_execute(&self) {
        unsafe {
            pthread_jit_write_protect_np(1);
            sys_icache_invalidate(self.ptr, self.code_len);
        }
    }

    /// Write compiled code into the buffer.
    pub fn write_code(&mut self, code: &[u32]) {
        let byte_len = code.len() * 4;
        assert!(byte_len <= self.size, "JIT code exceeds buffer size");
        self.enable_write();
        unsafe {
            ptr::copy_nonoverlapping(code.as_ptr() as *const u8, self.ptr, byte_len);
        }
        self.code_len = byte_len;
        self.enable_execute();
    }

    /// Get a function pointer to the compiled code.
    ///
    /// # Safety
    /// The caller must ensure valid code has been written via `write_code`
    /// and that `T` is a function pointer type matching its ABI.
    #[inline]
    pub unsafe fn as_fn<T>(&self) -> T
    where
        T: Copy,
    { unsafe {
        debug_assert!(std::mem::size_of::<T>() == std::mem::size_of::<usize>());
        std::mem::transmute_copy(&self.ptr)
    }}

    pub fn ptr(&self) -> *const u8 {
        self.ptr
    }
}

impl Drop for JitMemory {
    fn drop(&mut self) {
        unsafe {
            munmap(self.ptr, self.size);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_jit_memory_alloc_and_execute() {
        let mut mem = JitMemory::new(4096).expect("mmap failed");
        // Write a single RET instruction (0xD65F03C0) and call it
        let code = [0xD65F03C0u32];
        mem.write_code(&code);
        unsafe {
            let f: extern "C" fn() = mem.as_fn();
            f(); // should return without crashing
        }
    }

    #[test]
    fn test_jit_memory_return_value() {
        let mut mem = JitMemory::new(4096).expect("mmap failed");
        // MOV x0, #42; RET
        let code = [
            0xD2800540u32, // MOVZ x0, #42
            0xD65F03C0u32, // RET
        ];
        mem.write_code(&code);
        unsafe {
            let f: extern "C" fn() -> u64 = mem.as_fn();
            assert_eq!(f(), 42);
        }
    }
}
