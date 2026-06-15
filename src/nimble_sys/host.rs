use crate::data::*;

use super::{NimbleError, NimbleResult, bindings, return_code_to_result};

/// Returns a mutable reference to the global BLE host stack configuration.
pub fn ble_hs_cfg() -> &'static mut bindings::ble_hs_cfg {
    #[allow(static_mut_refs)]
    unsafe {
        &mut bindings::ble_hs_cfg
    }
}

/// Infers the best address type to use for the device, optionally using privacy.
///
/// # Arguments
///
/// * `privacy` - If true, privacy is enabled when inferring the address type.
///
/// # Returns
///
/// Returns the inferred address type as `u8` on success, or an error otherwise.
pub fn ble_hs_id_infer_auto(privacy: bool) -> NimbleResult<u8> {
    let mut addr_type = 0u8;
    let ret = unsafe { bindings::ble_hs_id_infer_auto(privacy as i32, &mut addr_type) };
    return_code_to_result(ret as u32, addr_type)
}

/// Copies the BLE device address for the given address type.
///
/// # Arguments
///
/// * `id_addr_type` - The address type to copy.
///
/// # Returns
///
/// Returns the BLE address as a `[u8; 6]` array on success, or an error otherwise.
pub fn ble_hs_id_copy_addr(id_addr_type: u8) -> NimbleResult<[u8; 6]> {
    let mut addr = [0; 6];

    let ret = unsafe {
        bindings::ble_hs_id_copy_addr(id_addr_type, addr.as_mut_ptr(), core::ptr::null_mut())
    };
    return_code_to_result(ret as u32, addr)
}

/// Parses advertisement fields from a discovery descriptor.
pub fn ble_hs_adv_parse_fields(
    disc: &bindings::ble_gap_disc_desc,
) -> NimbleResult<HostAdvertismentFields> {
    ble_hs_adv_parse_fields_slice(unsafe {
        core::slice::from_raw_parts(disc.data, disc.length_data as usize)
    })
}

/// Parses advertisement fields from a raw byte slice.
pub fn ble_hs_adv_parse_fields_slice(data: &[u8]) -> NimbleResult<HostAdvertismentFields> {
    let mut fields: bindings::ble_hs_adv_fields = unsafe { core::mem::zeroed() };
    let ret =
        unsafe { bindings::ble_hs_adv_parse_fields(&mut fields, data.as_ptr(), data.len() as u8) };
    return_code_to_result(ret as u32, fields.into())
}

/// Creates an mbuf from a flat data slice for BLE operations.
///
/// # Arguments
///
/// * `data` - The data slice to convert into an mbuf.
///
/// # Returns
///
/// Returns a pointer to the created `os_mbuf` on success, or an error otherwise.
pub fn ble_hs_mbuf_from_flat(data: &[u8]) -> NimbleResult<*mut bindings::os_mbuf> {
    let data_len = data.len();
    let data = data.as_ptr();
    let ptr = unsafe { bindings::ble_hs_mbuf_from_flat(data as *const _, data_len as u16) };

    if ptr.is_null() {
        return Err(NimbleError::MbufCreationFailed);
    }

    Ok(ptr)
}

/// Converts an mbuf to a flat byte vector.
///
/// # Arguments
///
/// * `om` - Pointer to the `os_mbuf` to convert.
///
/// # Returns
///
/// Returns a `Vec<u8>` containing the mbuf data on success, or an error otherwise.
pub fn ble_hs_mbuf_to_flat(om: *const bindings::os_mbuf) -> NimbleResult<alloc::vec::Vec<u8>> {
    let (ret, buffer) = unsafe {
        let om_ref = &*om;
        let buffer_length = om_ref.om_len;
        let mut buffer = alloc::vec![0u8; buffer_length as usize];
        let buffer_ptr = buffer.as_mut_ptr();
        let mut written_bytes = 0;
        (
            bindings::ble_hs_mbuf_to_flat(
                om,
                buffer_ptr as *mut _,
                buffer_length,
                &mut written_bytes,
            ),
            buffer,
        )
    };

    return_code_to_result(ret as u32, buffer)
}
