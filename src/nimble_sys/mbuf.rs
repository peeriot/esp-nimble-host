use super::{NimbleResult, bindings, return_code_to_result};

/// Allocates a pkthdr mbuf. Returns `None` if no memory is available.
pub fn os_msys_get_pkthdr(pkthdr_len: u16, user_hdr_len: u16) -> Option<*mut bindings::os_mbuf> {
    let om = unsafe { bindings::os_msys_get_pkthdr(pkthdr_len, user_hdr_len) };
    if om.is_null() { None } else { Some(om) }
}

/// Appends a byte slice to an mbuf chain.
pub fn os_mbuf_append_slice(om: *mut bindings::os_mbuf, data: &[u8]) -> NimbleResult {
    let rc = unsafe { bindings::os_mbuf_append(om, data.as_ptr().cast(), data.len() as u16) };
    return_code_to_result(rc as u32, ())
}

/// Frees an entire mbuf chain.
pub fn os_mbuf_free_chain(om: *mut bindings::os_mbuf) -> NimbleResult {
    let rc = unsafe { bindings::os_mbuf_free_chain(om) };
    return_code_to_result(rc as u32, ())
}
