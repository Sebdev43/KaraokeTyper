//! Returning per-dictation memory to the OS (Linux).
//!
//! Each dictation allocates multi-megabyte transient buffers that live for seconds:
//! the captured 16 kHz PCM and its padded copy on the Rust side, and the inference
//! engine's mel/FFT scratch on the C++ side. Freed quickly, but not necessarily
//! *returned* — allocators cache freed pages for reuse, and a few interleaved live
//! allocations are enough to pin them, so RSS grows by roughly the transient volume
//! of every dictation and then slowly migrates to swap.
//!
//! **This process has two allocators, and each needs its own treatment.**
//!
//! - **jemalloc** (`main.rs` installs `tikv_jemallocator` as the global allocator)
//!   serves every Rust allocation: the audio buffers, the transcript, everything the
//!   pipeline touches.
//! - **glibc malloc** still serves the C/C++ dependencies, which never go through
//!   Rust's `GlobalAlloc`: whisper.cpp via `transcribe-rs` and ONNX Runtime via `ort`.
//!   These are the largest transient allocators of the two.
//!
//! Handing back only one of them looks like a fix and reclaims half the memory, so
//! both entry points below address both allocators. Note the cfg predicates differ:
//! the glibc half is glibc-specific (`target_env = "gnu"`), the jemalloc half follows
//! whatever `main.rs` gates the global allocator on (`not(target_env = "msvc")`).

/// Pin glibc's mmap threshold so large C/C++ buffers keep taking the mmap path and
/// are handed straight back to the OS when freed.
///
/// glibc serves allocations above its "mmap threshold" with a private mmap that is
/// released on free — but the threshold is *dynamic*: freeing an mmapped block raises
/// it to that block's size, up to 32 MB. So after the first dictation every later
/// large buffer comes from a malloc arena instead, and stops being returned. Pinning
/// the threshold keeps the first-dictation behaviour for every dictation.
///
/// Must run before the workload allocates, hence the call at the top of `run()`.
#[cfg(all(target_os = "linux", target_env = "gnu"))]
pub(crate) fn init_allocator() {
    /// glibc's own starting threshold, before the dynamic bump. Large enough that
    /// ordinary small allocations keep using the arenas, small enough that every
    /// per-dictation PCM and mel buffer lands above it.
    const MMAP_THRESHOLD_BYTES: libc::c_int = 128 * 1024;

    // SAFETY: FFI call with no memory arguments; mallopt only updates malloc's
    // internal parameters.
    let ok = unsafe { libc::mallopt(libc::M_MMAP_THRESHOLD, MMAP_THRESHOLD_BYTES) };
    if ok == 0 {
        crate::dev_log!("[MEM] mallopt(M_MMAP_THRESHOLD) refused");
    }
}

#[cfg(not(all(target_os = "linux", target_env = "gnu")))]
pub(crate) fn init_allocator() {}

/// Return freed-but-cached memory to the OS, on both allocators.
///
/// Called once per finished transcription, off the async workers — it takes on the
/// order of a millisecond.
pub(crate) fn trim_freed_memory() {
    trim_glibc();
    if purge_jemalloc() != 0 {
        crate::dev_log!("[MEM] jemalloc arena purge refused");
    }
}

/// glibc arenas — the C/C++ inference libraries' churn below the mmap threshold.
#[cfg(all(target_os = "linux", target_env = "gnu"))]
fn trim_glibc() {
    // SAFETY: FFI call with no memory arguments; malloc_trim releases whole free
    // pages via madvise and is thread-safe.
    unsafe {
        libc::malloc_trim(0);
    }
}

#[cfg(not(all(target_os = "linux", target_env = "gnu")))]
fn trim_glibc() {}

/// jemalloc arenas — every Rust-side buffer, which `malloc_trim` cannot see.
///
/// Returns jemalloc's status code so the caller (today: the test) can tell a
/// successful purge from a silently mistyped mallctl name.
#[cfg(not(target_env = "msvc"))]
fn purge_jemalloc() -> i32 {
    // `arena.<i>.purge` with i = MALLCTL_ARENAS_ALL purges every arena at once.
    const PURGE_ALL_ARENAS: &[u8] = b"arena.4096.purge\0";

    // SAFETY: the name is a NUL-terminated C string with 'static lifetime, and all
    // four value pointers are null, which mallctl documents as "no input, no output"
    // for a write-only control. mallctl is thread-safe.
    unsafe {
        tikv_jemalloc_sys::mallctl(
            PURGE_ALL_ARENAS.as_ptr().cast(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            0,
        )
    }
}

#[cfg(target_env = "msvc")]
fn purge_jemalloc() -> i32 {
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entry_points_are_callable() {
        init_allocator();
        trim_freed_memory();
    }

    /// Guards the mallctl name: a typo returns ENOENT instead of purging, and would
    /// otherwise be invisible — the memory just never comes back.
    #[cfg(not(target_env = "msvc"))]
    #[test]
    fn jemalloc_purge_is_accepted() {
        assert_eq!(
            purge_jemalloc(),
            0,
            "mallctl rejected the purge control name"
        );
    }
}
