// JIT memory management for aarch64 — macOS and Linux.
//
// Both platforms hand out one W^X-managed mmap region per compiled program,
// but they reach W^X by different mechanisms:
//
//   * **macOS** maps the region `RWX` with `MAP_JIT` and flips the *thread's*
//     JIT write-protect state with `pthread_jit_write_protect_np`, then
//     flushes with `sys_icache_invalidate`. The permission flip is per-thread
//     and process-global across every `MAP_JIT` region the thread owns.
//   * **Linux** has no `MAP_JIT`: the region is mapped `RW` (never `RWX`) and
//     `mprotect` swaps it between `R|W` and `R|X`. That flip is *per region*,
//     and the I-cache is cleared with `__clear_cache`.
//
// Because the two mechanisms have different scopes, `enable_write` /
// `enable_execute` are private: `write_code` is their only caller, so the
// difference cannot leak into the compiler.

use std::ptr;

/// Size of JIT code buffer per program (64KB — generous headroom over ~16KB typical)
pub const JIT_CODE_SIZE: usize = 64 * 1024;

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
compile_error!(
    "randomx::jit is gated on target_arch = \"aarch64\" but its memory backend \
     supports only macOS and Linux. Add a `platform` arm in \
     src/randomx/jit/memory.rs for this OS, or build with the JIT excluded. \
     (Note aarch64-linux-android is target_os = \"android\" and lands here.)"
);

// ============================================================================
// macOS backend — MAP_JIT + pthread_jit_write_protect_np
// ============================================================================
#[cfg(target_os = "macos")]
mod platform {
    use std::ptr;

    unsafe extern "C" {
        fn pthread_jit_write_protect_np(enabled: i32);
        fn sys_icache_invalidate(start: *mut u8, size: usize);
    }

    // <sys/mman.h>, Darwin.
    const MAP_ANON: i32 = 0x1000;
    const MAP_PRIVATE: i32 = 0x0002;
    const MAP_JIT: i32 = 0x0800;
    const PROT_READ: i32 = 0x01;
    const PROT_WRITE: i32 = 0x02;
    const PROT_EXEC: i32 = 0x04;
    const MAP_FAILED: *mut u8 = !0 as *mut u8;

    unsafe extern "C" {
        fn mmap(addr: *mut u8, len: usize, prot: i32, flags: i32, fd: i32, offset: i64)
            -> *mut u8;
        fn munmap(addr: *mut u8, len: usize) -> i32;
    }

    pub(super) fn alloc(size: usize) -> Result<*mut u8, &'static str> {
        let p = unsafe {
            mmap(
                ptr::null_mut(),
                size,
                PROT_READ | PROT_WRITE | PROT_EXEC,
                MAP_ANON | MAP_PRIVATE | MAP_JIT,
                -1,
                0,
            )
        };
        if p == MAP_FAILED || p.is_null() {
            return Err("mmap MAP_JIT failed");
        }
        Ok(p)
    }

    pub(super) fn dealloc(p: *mut u8, size: usize) {
        unsafe { munmap(p, size) };
    }

    /// Darwin's toggle is per-thread and covers every `MAP_JIT` region, so the
    /// region's own address and length are not needed here.
    #[inline]
    pub(super) fn enable_write(_p: *mut u8, _size: usize) {
        unsafe { pthread_jit_write_protect_np(0) };
    }

    #[inline]
    pub(super) fn enable_execute(p: *mut u8, _size: usize, code_len: usize) {
        unsafe {
            pthread_jit_write_protect_np(1);
            sys_icache_invalidate(p, code_len);
        }
    }
}

// ============================================================================
// Linux backend — mmap(RW) / mprotect(RX) + __clear_cache
// ============================================================================
#[cfg(target_os = "linux")]
mod platform {
    use std::ptr;

    // <sys/mman.h>, Linux. These differ from Darwin's: MAP_ANONYMOUS is 0x20
    // (Darwin's MAP_ANON is 0x1000) and there is no MAP_JIT — 0x0800 is
    // MAP_DENYWRITE here, which is exactly the class of silent runtime bug the
    // separate backends exist to prevent.
    const MAP_ANONYMOUS: i32 = 0x20;
    const MAP_PRIVATE: i32 = 0x02;
    const PROT_READ: i32 = 0x1;
    const PROT_WRITE: i32 = 0x2;
    const PROT_EXEC: i32 = 0x4;
    const MAP_FAILED: *mut u8 = !0 as *mut u8;

    unsafe extern "C" {
        fn mmap(addr: *mut u8, len: usize, prot: i32, flags: i32, fd: i32, offset: i64)
            -> *mut u8;
        fn munmap(addr: *mut u8, len: usize) -> i32;
        fn mprotect(addr: *mut u8, len: usize, prot: i32) -> i32;
        /// Provided by libgcc/compiler-rt; issues the `dc cvau` / `ic ivau` /
        /// `isb` sequence AArch64 needs to make freshly written bytes visible
        /// to the instruction fetcher.
        fn __clear_cache(start: *mut u8, end: *mut u8);
    }

    /// Mapped write-only-plus-read: the region is never simultaneously
    /// writable and executable.
    pub(super) fn alloc(size: usize) -> Result<*mut u8, &'static str> {
        let p = unsafe {
            mmap(
                ptr::null_mut(),
                size,
                PROT_READ | PROT_WRITE,
                MAP_ANONYMOUS | MAP_PRIVATE,
                -1,
                0,
            )
        };
        if p == MAP_FAILED || p.is_null() {
            return Err("mmap PROT_READ|PROT_WRITE failed");
        }
        Ok(p)
    }

    pub(super) fn dealloc(p: *mut u8, size: usize) {
        unsafe { munmap(p, size) };
    }

    /// An unchecked `mprotect` would surface as a bare SIGSEGV inside emitted
    /// code with no context, so both directions panic loudly instead.
    fn protect(p: *mut u8, size: usize, prot: i32, what: &str) {
        let rc = unsafe { mprotect(p, size, prot) };
        assert!(
            rc == 0,
            "mprotect({what}) on the JIT region failed: {}",
            std::io::Error::last_os_error()
        );
    }

    #[inline]
    pub(super) fn enable_write(p: *mut u8, size: usize) {
        protect(p, size, PROT_READ | PROT_WRITE, "PROT_READ|PROT_WRITE");
    }

    #[inline]
    pub(super) fn enable_execute(p: *mut u8, size: usize, code_len: usize) {
        protect(p, size, PROT_READ | PROT_EXEC, "PROT_READ|PROT_EXEC");
        unsafe { __clear_cache(p, p.add(code_len)) };
    }
}

pub struct JitMemory {
    ptr: *mut u8,
    size: usize,
    code_len: usize, // actual bytes written
}

unsafe impl Send for JitMemory {}

impl JitMemory {
    /// Allocate a new JIT memory region.
    pub fn new(size: usize) -> Result<Self, &'static str> {
        let ptr = platform::alloc(size)?;
        Ok(JitMemory {
            ptr,
            size,
            code_len: 0,
        })
    }

    /// Enable writing to JIT memory (disables execute permission).
    #[inline]
    fn enable_write(&self) {
        platform::enable_write(self.ptr, self.size);
    }

    /// Enable execute permission (disables write).
    /// Also flushes the instruction cache.
    #[inline]
    fn enable_execute(&self) {
        platform::enable_execute(self.ptr, self.size, self.code_len);
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
        platform::dealloc(self.ptr, self.size);
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

    /// A region is rewritten in place by the two-pass native-loop compile
    /// (`compiler.rs` writes the body, then rewrites with the loop), and by a
    /// `JitCompiler` reused across programs. The second body must actually
    /// execute — on Linux that needs both the `mprotect` round-trip and the
    /// I-cache clear to be correct, and a stale I-cache line would return the
    /// *first* value here.
    #[test]
    fn test_jit_memory_rewrite_in_place() {
        let mut mem = JitMemory::new(4096).expect("mmap failed");
        mem.write_code(&[0xD2800540u32, 0xD65F03C0u32]); // MOVZ x0, #42; RET
        unsafe {
            let f: extern "C" fn() -> u64 = mem.as_fn();
            assert_eq!(f(), 42);
        }
        mem.write_code(&[0xD28006E0u32, 0xD65F03C0u32]); // MOVZ x0, #55; RET
        unsafe {
            let f: extern "C" fn() -> u64 = mem.as_fn();
            assert_eq!(f(), 55, "rewritten code must execute, not a stale I-cache line");
        }
    }
}
