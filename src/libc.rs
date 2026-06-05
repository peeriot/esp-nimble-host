use core::ffi::{c_char, c_size_t};

/// # Safety
/// Caller must ensure `dest` has enough capacity for `strlen(dest) + strlen(src) + 1`
/// bytes and that both pointers are valid, non-overlapping, null-terminated C strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn strncat(
    dest: *mut c_char,
    src: *const c_char,
    num: c_size_t,
) -> *const c_char {
    unsafe { tinyrlibc::strcpy(dest.add(num.min(tinyrlibc::strlen(dest))), src) };
    dest
}
