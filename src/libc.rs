use core::ffi::{c_char, c_size_t};

#[unsafe(no_mangle)]
pub unsafe extern "C" fn strncat(
    dest: *mut c_char,
    src: *const c_char,
    num: c_size_t,
) -> *const c_char {
    unsafe { tinyrlibc::strcpy(dest.add(num.min(tinyrlibc::strlen(dest))), src) };
    dest
}
