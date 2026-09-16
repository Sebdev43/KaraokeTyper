//! Forwarders for the glibc 2.38 `strtol` family.
//!
//! ONNX Runtime's prebuilt static library (ort-sys `download-binaries`, 1.24.2) is
//! built against glibc ≥ 2.38, where `strtol` & co. were renamed `__isoc23_strto*`.
//! The Linux release is linked on Ubuntu 22.04 (glibc 2.35) so the .deb/AppImage
//! keep running there, and that libc has no such symbols: these provide them.
//! C23 only changed how a "0b"/"0B" prefix is read, which ONNX Runtime's option
//! parsing never relies on. On a newer glibc the executable's definitions simply
//! take precedence, with identical behaviour.
#![cfg(all(target_os = "linux", target_env = "gnu"))]

use std::ffi::{c_char, c_int, c_long, c_longlong, c_ulong, c_ulonglong};

/// # Safety
/// Same contract as `strtol(3)`: `s` points to a NUL-terminated string and `end`,
/// when non-null, to writable storage for a pointer.
#[no_mangle]
pub unsafe extern "C" fn __isoc23_strtol(
    s: *const c_char,
    end: *mut *mut c_char,
    base: c_int,
) -> c_long {
    // SAFETY: forwarded verbatim; the caller upholds the strtol(3) contract.
    unsafe { libc::strtol(s, end, base) }
}

/// # Safety
/// Same contract as `strtoll(3)`.
#[no_mangle]
pub unsafe extern "C" fn __isoc23_strtoll(
    s: *const c_char,
    end: *mut *mut c_char,
    base: c_int,
) -> c_longlong {
    // SAFETY: forwarded verbatim; the caller upholds the strtoll(3) contract.
    unsafe { libc::strtoll(s, end, base) }
}

/// # Safety
/// Same contract as `strtoul(3)`.
#[no_mangle]
pub unsafe extern "C" fn __isoc23_strtoul(
    s: *const c_char,
    end: *mut *mut c_char,
    base: c_int,
) -> c_ulong {
    // SAFETY: forwarded verbatim; the caller upholds the strtoul(3) contract.
    unsafe { libc::strtoul(s, end, base) }
}

/// # Safety
/// Same contract as `strtoull(3)`.
#[no_mangle]
pub unsafe extern "C" fn __isoc23_strtoull(
    s: *const c_char,
    end: *mut *mut c_char,
    base: c_int,
) -> c_ulonglong {
    // SAFETY: forwarded verbatim; the caller upholds the strtoull(3) contract.
    unsafe { libc::strtoull(s, end, base) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CString;

    #[test]
    fn forwarders_parse_like_the_libc_originals() {
        let s = CString::new("  -42xyz").expect("no NUL");
        let mut end: *mut c_char = std::ptr::null_mut();
        // SAFETY: valid NUL-terminated input and a valid out-pointer.
        let (v, rest) = unsafe {
            let v = __isoc23_strtol(s.as_ptr(), &mut end, 10);
            (
                v,
                std::ffi::CStr::from_ptr(end).to_string_lossy().into_owned(),
            )
        };
        assert_eq!(v, -42);
        assert_eq!(rest, "xyz");

        let hex = CString::new("ff").expect("no NUL");
        // SAFETY: valid input, `end` unused.
        let (ll, ul, ull) = unsafe {
            (
                __isoc23_strtoll(hex.as_ptr(), std::ptr::null_mut(), 16),
                __isoc23_strtoul(hex.as_ptr(), std::ptr::null_mut(), 16),
                __isoc23_strtoull(hex.as_ptr(), std::ptr::null_mut(), 16),
            )
        };
        assert_eq!((ll, ul, ull), (255, 255, 255));
    }
}
