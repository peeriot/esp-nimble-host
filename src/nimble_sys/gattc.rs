use crate::{data::*};

use super::{bindings, NimbleResult, host::ble_hs_mbuf_from_flat, return_code_to_result};

/// Reads a GATT characteristic value from the specified connection handle.
///
/// # Arguments
///
/// * `conn_handle` - The connection handle.
/// * `attr_handle` - The attribute handle of the characteristic to read.
/// * `cb` - The callback function to call when the read operation completes.
/// * `cb_arg` - An optional argument to pass to the callback.
///
/// # Returns
///
/// Returns a `NimbleResult` indicating success or failure.
pub fn ble_gattc_read(
    conn_handle: ConnectionHandle,
    attr_handle: u16,
    cb: bindings::ble_gatt_attr_fn,
    cb_arg: *mut core::ffi::c_void,
) -> NimbleResult {
    let ret = unsafe { bindings::ble_gattc_read(conn_handle, attr_handle, cb, cb_arg) };

    return_code_to_result(ret as u32, ())
}

/// Discovers all primary services on the specified connection.
///
/// # Arguments
///
/// * `conn_handle` - The connection handle.
/// * `cb` - The callback function to call for each discovered service.
/// * `cb_arg` - An optional argument to pass to the callback.
///
/// # Returns
///
/// Returns a `NimbleResult` indicating success or failure.
pub fn ble_gattc_disc_all_svcs(
    conn_handle: ConnectionHandle,
    cb: bindings::ble_gatt_disc_svc_fn,
    cb_arg: *mut core::ffi::c_void,
) -> NimbleResult {
    let ret = unsafe { bindings::ble_gattc_disc_all_svcs(conn_handle, cb, cb_arg) };

    return_code_to_result(ret as u32, ())
}

/// Discovers primary services by UUID on the specified connection.
///
/// # Arguments
///
/// * `conn_handle` - The connection handle.
/// * `uuid` - The UUID of the service to discover.
/// * `cb` - The callback function to call for each discovered service.
/// * `cb_arg` - An optional argument to pass to the callback.
///
/// # Returns
///
/// Returns a `NimbleResult` indicating success or failure.
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

/// Discovers all characteristics within a handle range on the specified connection.
///
/// # Arguments
///
/// * `conn_handle` - The connection handle.
/// * `start_handle` - The start of the handle range.
/// * `end_handle` - The end of the handle range.
/// * `cb` - The callback function to call for each discovered characteristic.
/// * `cb_arg` - An optional argument to pass to the callback.
///
/// # Returns
///
/// Returns a `NimbleResult` indicating success or failure.
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

/// Discovers all descriptors within a handle range on the specified connection.
///
/// # Arguments
///
/// * `conn_handle` - The connection handle.
/// * `start_handle` - The start of the handle range.
/// * `end_handle` - The end of the handle range.
/// * `cb` - The callback function to call for each discovered descriptor.
/// * `cb_arg` - An optional argument to pass to the callback.
///
/// # Returns
///
/// Returns a `NimbleResult` indicating success or failure.
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

/// Writes a value to a characteristic without response using a flat buffer.
///
/// # Arguments
///
/// * `conn_handle` - The connection handle.
/// * `attr_handle` - The attribute handle to write to.
/// * `data` - The data to write.
///
/// # Returns
///
/// Returns a `NimbleResult` indicating success or failure.
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

/// Writes a value to a characteristic using a flat buffer and invokes a callback on completion.
///
/// # Arguments
///
/// * `conn_handle` - The connection handle.
/// * `attr_handle` - The attribute handle to write to.
/// * `data` - The data to write.
/// * `cb` - The callback function to call when the write operation completes.
/// * `cb_arg` - An optional argument to pass to the callback.
///
/// # Returns
///
/// Returns a `NimbleResult` indicating success or failure.
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

/// Writes a long value to a characteristic at a given offset and invokes a callback on completion.
///
/// # Arguments
///
/// * `conn_handle` - The connection handle.
/// * `attr_handle` - The attribute handle to write to.
/// * `offset` - The offset at which to write the data.
/// * `data` - The data to write.
/// * `cb` - The callback function to call when the write operation completes.
/// * `cb_arg` - An optional argument to pass to the callback.
///
/// # Returns
///
/// Returns a `NimbleResult` indicating success or failure.
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

/// Exchanges the ATT MTU with the peer device.
///
/// # Arguments
///
/// * `conn_handle` - The connection handle.
/// * `cb` - The callback function to call when the MTU exchange completes.
/// * `cb_arg` - An optional argument to pass to the callback.
///
/// # Returns
///
/// Returns a `NimbleResult` indicating success or failure.
pub fn ble_gattc_exchange_mtu(
    conn_handle: ConnectionHandle,
    cb: bindings::ble_gatt_mtu_fn,
    cb_arg: *mut core::ffi::c_void,
) -> NimbleResult {
    let ret = unsafe { bindings::ble_gattc_exchange_mtu(conn_handle, cb, cb_arg) };

    return_code_to_result(ret as u32, ())
}
