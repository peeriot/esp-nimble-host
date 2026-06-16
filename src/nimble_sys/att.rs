use core::num::NonZeroU16;

use crate::{data::ConnectionHandle, nimble_sys::return_code_to_result};

use super::{NimbleError, NimbleResult, bindings};

/// Returns the ATT MTU for a connection, or an error if it is zero.
pub fn ble_att_mtu(conn_handle: ConnectionHandle) -> NimbleResult<NonZeroU16> {
    let mtu = unsafe { bindings::ble_att_mtu(conn_handle) };

    NonZeroU16::new(mtu).ok_or(NimbleError::AttMtuZero(conn_handle))
}

/// Sets the preferred ATT MTU for future connections.
pub fn ble_att_set_preferred_mtu(mtu: u16) -> NimbleResult {
    let ret = unsafe { bindings::ble_att_set_preferred_mtu(mtu) };

    return_code_to_result(ret as u32, ())
}
