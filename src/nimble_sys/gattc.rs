use crate::data::*;

use super::{NimbleResult, bindings, host::ble_hs_mbuf_from_flat, return_code_to_result};

/// Reads a GATT characteristic value.
pub fn ble_gattc_read(
    conn_handle: ConnectionHandle,
    attr_handle: u16,
    cb: bindings::ble_gatt_attr_fn,
    cb_arg: *mut core::ffi::c_void,
) -> NimbleResult {
    let ret = unsafe { bindings::ble_gattc_read(conn_handle, attr_handle, cb, cb_arg) };

    return_code_to_result(ret as u32, ())
}

/// Discovers all primary services on a connection.
pub fn ble_gattc_disc_all_svcs(
    conn_handle: ConnectionHandle,
    cb: bindings::ble_gatt_disc_svc_fn,
    cb_arg: *mut core::ffi::c_void,
) -> NimbleResult {
    let ret = unsafe { bindings::ble_gattc_disc_all_svcs(conn_handle, cb, cb_arg) };

    return_code_to_result(ret as u32, ())
}

/// Discovers primary services matching a UUID on a connection.
pub fn ble_gattc_disc_svc_by_uuid(
    conn_handle: ConnectionHandle,
    uuid: &NimbleUuid,
    cb: bindings::ble_gatt_disc_svc_fn,
    cb_arg: *mut core::ffi::c_void,
) -> NimbleResult {
    let ret =
        unsafe { bindings::ble_gattc_disc_svc_by_uuid(conn_handle, uuid.raw_ptr(), cb, cb_arg) };

    return_code_to_result(ret as u32, ())
}

/// Discovers all characteristics within a handle range.
pub fn ble_gattc_disc_all_chrs(
    conn_handle: ConnectionHandle,
    start_handle: u16,
    end_handle: u16,
    cb: bindings::ble_gatt_chr_fn,
    cb_arg: *mut core::ffi::c_void,
) -> NimbleResult {
    let ret = unsafe {
        bindings::ble_gattc_disc_all_chrs(conn_handle, start_handle, end_handle, cb, cb_arg)
    };

    return_code_to_result(ret as u32, ())
}

/// Discovers all descriptors within a handle range.
pub fn ble_gattc_disc_all_dscs(
    conn_handle: ConnectionHandle,
    start_handle: u16,
    end_handle: u16,
    cb: bindings::ble_gatt_dsc_fn,
    cb_arg: *mut core::ffi::c_void,
) -> NimbleResult {
    let ret = unsafe {
        bindings::ble_gattc_disc_all_dscs(conn_handle, start_handle, end_handle, cb, cb_arg)
    };

    return_code_to_result(ret as u32, ())
}

/// Writes a characteristic value without response (fire-and-forget).
pub fn ble_gattc_write_no_rsp_flat(
    conn_handle: ConnectionHandle,
    attr_handle: u16,
    data: &[u8],
) -> NimbleResult {
    let data_len = data.len();
    let data = data.as_ptr();
    let ret = unsafe {
        bindings::ble_gattc_write_no_rsp_flat(
            conn_handle,
            attr_handle,
            data as *const _,
            data_len as u16,
        )
    };

    return_code_to_result(ret as u32, ())
}

/// Writes a characteristic value with response using a flat buffer.
pub fn ble_gattc_write_flat(
    conn_handle: ConnectionHandle,
    attr_handle: u16,
    data: &[u8],
    cb: bindings::ble_gatt_attr_fn,
    cb_arg: *mut core::ffi::c_void,
) -> NimbleResult {
    let data_len = data.len();
    let data = data.as_ptr();
    let ret = unsafe {
        bindings::ble_gattc_write_flat(
            conn_handle,
            attr_handle,
            data as *const _,
            data_len as u16,
            cb,
            cb_arg,
        )
    };

    return_code_to_result(ret as u32, ())
}

/// Writes a long characteristic value using the Prepare Write / Execute sequence.
pub fn ble_gattc_write_long(
    conn_handle: ConnectionHandle,
    attr_handle: u16,
    offset: u16,
    data: &[u8],
    cb: bindings::ble_gatt_attr_fn,
    cb_arg: *mut core::ffi::c_void,
) -> NimbleResult {
    let om = ble_hs_mbuf_from_flat(data)?;
    let ret =
        unsafe { bindings::ble_gattc_write_long(conn_handle, attr_handle, offset, om, cb, cb_arg) };

    return_code_to_result(ret as u32, ())
}

/// Initiates an ATT MTU exchange with the peer.
pub fn ble_gattc_exchange_mtu(
    conn_handle: ConnectionHandle,
    cb: bindings::ble_gatt_mtu_fn,
    cb_arg: *mut core::ffi::c_void,
) -> NimbleResult {
    let ret = unsafe { bindings::ble_gattc_exchange_mtu(conn_handle, cb, cb_arg) };

    return_code_to_result(ret as u32, ())
}
