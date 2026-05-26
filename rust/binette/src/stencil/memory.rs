use super::*;

pub(super) struct ExecutableMemory {
    ptr: NonNull<u8>,
    len: usize,
}

// SAFETY: after construction the mapping is RX and immutable; Drop only unmaps
// once ownership ends.
unsafe impl Send for ExecutableMemory {}
// SAFETY: callers only execute immutable code bytes through shared references.
unsafe impl Sync for ExecutableMemory {}

impl ExecutableMemory {
    pub(super) fn new(code: &[u8]) -> Result<Self, StencilError> {
        let len = code.len().max(1);
        let flags = libc::MAP_PRIVATE | libc::MAP_ANON | map_jit_flag();
        // SAFETY: mmap is called with a null hint, anonymous fd, and checked result.
        let ptr = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                len,
                libc::PROT_READ | libc::PROT_WRITE,
                flags,
                -1,
                0,
            )
        };
        if ptr == libc::MAP_FAILED {
            return Err(StencilError::ExecutableMemory);
        }

        // SAFETY: ptr is a valid writable mapping of at least len bytes.
        unsafe {
            copy_nonoverlapping(code.as_ptr(), ptr.cast::<u8>(), code.len());
            flush_instruction_cache(ptr, code.len());
            if libc::mprotect(ptr, len, libc::PROT_READ | libc::PROT_EXEC) != 0 {
                let _ = libc::munmap(ptr, len);
                return Err(StencilError::Mprotect);
            }
        }

        let Some(ptr) = NonNull::new(ptr.cast::<u8>()) else {
            // SAFETY: ptr/len are the live mapping returned by mmap.
            unsafe {
                let _ = libc::munmap(ptr, len);
            }
            return Err(StencilError::ExecutableMemory);
        };

        Ok(Self { ptr, len })
    }

    pub(super) fn len(&self) -> usize {
        self.len
    }

    pub(super) fn as_fixed_fn(&self) -> FixedStencilFn {
        // SAFETY: the mapping contains generated code ending in ret and has RX
        // permissions for the lifetime of self.
        unsafe { std::mem::transmute(self.ptr.as_ptr()) }
    }

    pub(super) fn as_hybrid_fn(&self) -> HybridStencilFn {
        // SAFETY: the mapping contains generated code ending in ret and has RX
        // permissions for the lifetime of self.
        unsafe { std::mem::transmute(self.ptr.as_ptr()) }
    }

    pub(super) fn as_encode_fn(&self) -> EncodeStencilFn {
        // SAFETY: the mapping contains generated code ending in ret and has RX
        // permissions for the lifetime of self.
        unsafe { std::mem::transmute(self.ptr.as_ptr()) }
    }

    pub(super) fn as_direct_encode_fn(&self) -> DirectEncodeStencilFn {
        // SAFETY: the mapping contains generated code ending in ret and has RX
        // permissions for the lifetime of self.
        unsafe { std::mem::transmute(self.ptr.as_ptr()) }
    }
}

impl Drop for ExecutableMemory {
    fn drop(&mut self) {
        // SAFETY: ptr/len are the live mapping returned by mmap.
        unsafe {
            let _ = libc::munmap(self.ptr.as_ptr().cast::<c_void>(), self.len);
        }
    }
}

#[cfg(target_os = "macos")]
fn map_jit_flag() -> i32 {
    libc::MAP_JIT
}

#[cfg(not(target_os = "macos"))]
fn map_jit_flag() -> i32 {
    0
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
unsafe fn flush_instruction_cache(ptr: *mut c_void, len: usize) {
    unsafe extern "C" {
        fn sys_icache_invalidate(start: *mut c_void, len: usize);
    }
    // SAFETY: caller provides the writable code range that was just populated.
    unsafe { sys_icache_invalidate(ptr, len) };
}

#[cfg(not(any(target_os = "macos", target_os = "ios")))]
unsafe fn flush_instruction_cache(_ptr: *mut c_void, _len: usize) {}
