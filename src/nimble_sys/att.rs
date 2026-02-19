use core::num::NonZeroU16;

use crate::{data::ConnectionHandle, nimble_sys::return_code_to_result};

use super::{NimbleError, NimbleResult, bindings};

/// Retrieves the ATT MTU for a given connection handle.
///
/// # Arguments
///
/// * `conn_handle` - The connection handle for which to retrieve the MTU.
///
/// # Returns
///
/// Returns the MTU size as `NonZeroU16` if successful, or an error if the MTU is zero.
pub fn ble_att_mtu(conn_handle: ConnectionHandle) -> NimbleResult<NonZeroU16> {
    let mtu = unsafe { bindings::ble_att_mtu(conn_handle) };

    NonZeroU16::new(mtu).ok_or_else(|| NimbleError::AttMtuZero(conn_handle))
}

/// Sets the preferred ATT MTU for future connections.
///
/// # Arguments
///
/// * `mtu` - The preferred MTU size to set.
///
/// # Returns
///
/// Returns `Ok(())` if successful, or an error if the operation fails.
pub fn ble_att_set_preferred_mtu(mtu: u16) -> NimbleResult {
    let ret = unsafe { bindings::ble_att_set_preferred_mtu(mtu) };

    return_code_to_result(ret as u32, ())
}
