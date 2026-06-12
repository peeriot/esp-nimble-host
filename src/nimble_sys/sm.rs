use super::{NimbleResult, bindings, return_code_to_result};

pub fn ble_gap_security_initiate(conn_handle: u16) -> NimbleResult {
    let ret = unsafe { bindings::ble_gap_security_initiate(conn_handle) };
    return_code_to_result(ret as u32, ())
}

pub fn ble_sm_inject_io(conn_handle: u16, io: &mut bindings::ble_sm_io) -> NimbleResult {
    let ret = unsafe { bindings::ble_sm_inject_io(conn_handle, io) };
    return_code_to_result(ret as u32, ())
}
