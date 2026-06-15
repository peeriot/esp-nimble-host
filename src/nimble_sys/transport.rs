use core::ffi::c_void;

use super::{NimbleError, NimbleResult, bindings, return_code_to_result};

/// Allocates a buffer for an HCI event from the transport layer.
///
/// Returns `None` if no memory is currently available; the caller should
/// retry (e.g. yield and loop) until allocation succeeds.
pub fn ble_transport_alloc_evt() -> Option<*mut bindings::ble_hci_ev> {
    let ev = unsafe { bindings::ble_transport_alloc_evt(0) };
    if ev.is_null() {
        None
    } else {
        Some(ev as *mut bindings::ble_hci_ev)
    }
}

/// Frees a buffer allocated by the NimBLE transport layer.
pub fn ble_transport_free(buf: *mut c_void) {
    unsafe { bindings::ble_transport_free(buf) }
}

/// Delivers an ACL mbuf to the NimBLE host stack.
///
/// On failure the mbuf is **not** freed; the caller is responsible for cleanup.
pub fn ble_transport_to_hs_acl(om: *mut bindings::os_mbuf) -> NimbleResult {
    let rc = unsafe { bindings::ble_transport_to_hs_acl_impl(om) };
    return_code_to_result(rc as u32, ())
}

/// Writes fields into an HCI event buffer and delivers it to the NimBLE host.
///
/// On failure the buffer is freed automatically and `Err` is returned.
/// `payload.len()` must fit in a `u8`.
pub fn ble_transport_to_hs_evt(
    ev: *mut bindings::ble_hci_ev,
    opcode: u8,
    payload: &[u8],
) -> NimbleResult {
    unsafe {
        (*ev).opcode = opcode;
        (*ev).length = payload.len() as u8;
        (*ev)
            .data
            .as_mut_slice(payload.len())
            .copy_from_slice(payload);
    }
    let rc = unsafe { bindings::ble_transport_to_hs_evt_impl(ev as *mut c_void) };
    if rc != 0 {
        ble_transport_free(ev as *mut c_void);
        return Err(NimbleError::OsError);
    }
    Ok(())
}
