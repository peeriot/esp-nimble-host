use super::{NimbleResult, bindings, return_code_to_result};

pub fn ble_gap_security_initiate(conn_handle: u16) -> NimbleResult {
    let ret = unsafe { bindings::ble_gap_security_initiate(conn_handle) };
    return_code_to_result(ret as u32, ())
}

#[cfg(nimble_sm)]
pub fn ble_sm_inject_io(conn_handle: u16, io: &mut bindings::ble_sm_io) -> NimbleResult {
    let ret = unsafe { bindings::ble_sm_inject_io(conn_handle, io) };
    return_code_to_result(ret as u32, ())
}

/// Without the security manager NimBLE replaces `ble_sm_inject_io` with a macro
/// returning `BLE_HS_ENOTSUP`, which bindgen cannot bind; mirror it here.
#[cfg(not(nimble_sm))]
pub fn ble_sm_inject_io(_conn_handle: u16, _io: &mut bindings::ble_sm_io) -> NimbleResult {
    return_code_to_result(bindings::BLE_HS_ENOTSUP, ())
}

/// Constructs a `ble_sm_io` for passkey input/display actions.
pub fn ble_sm_io_passkey(action: u8, passkey: u32) -> bindings::ble_sm_io {
    unsafe {
        let mut io: bindings::ble_sm_io = core::mem::zeroed();
        io.action = action;
        io.__bindgen_anon_1.passkey = passkey;
        io
    }
}
