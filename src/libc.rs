use core::ffi::{c_char, c_size_t};

/// # Safety
/// Caller must ensure `dest` has enough capacity for `strlen(dest) + num + 1`
/// bytes and that both pointers are valid, non-overlapping, null-terminated C strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn strncat(
    dest: *mut c_char,
    src: *const c_char,
    num: c_size_t,
) -> *const c_char {
    unsafe {
        let mut d = dest.add(tinyrlibc::strlen(dest));
        let mut s = src;
        let mut remaining = num;
        while remaining > 0 {
            let byte = *s;
            if byte == 0 {
                break;
            }
            *d = byte;
            d = d.add(1);
            s = s.add(1);
            remaining -= 1;
        }
        *d = 0;
    }
    dest
}
