// use core::ffi::CString;

use crate::data::*;

use super::{NimbleResult, bindings, return_code_to_result};

/// Cancels an ongoing BLE GAP discovery process.
///
/// # Returns
///
/// Returns a `NimbleResult` indicating success or failure.
pub fn ble_gap_disc_cancel() -> NimbleResult {
    let ret = unsafe { bindings::ble_gap_disc_cancel() };
    return_code_to_result(ret as u32, ())
}

/// Starts a BLE GAP discovery process.
///
/// # Arguments
///
/// * `own_addr_type` - The address type of the local device.
/// * `duration_ms` - Duration of the discovery in milliseconds.
/// * `disc_params` - Parameters for the discovery process.
/// * `cb` - Callback function to handle events.
/// * `cb_arg` - Argument to pass to the callback function.
///
/// # Returns
///
/// Returns a `NimbleResult` indicating success or failure.
pub fn ble_gap_disc(
    own_addr_type: u8,
    duration_ms: u32,
    disc_params: &BleGapDiscParams,
    cb: bindings::ble_gap_event_fn,
    cb_arg: *mut core::ffi::c_void,
) -> NimbleResult {
    let ret = unsafe {
        bindings::ble_gap_disc(
            own_addr_type,
            duration_ms as _,
            disc_params.inner(),
            cb,
            cb_arg,
        )
    };
    return_code_to_result(ret as u32, ())
}

/// Initiates a BLE GAP connection to a peer device.
///
/// # Arguments
///
/// * `own_addr_type` - The address type of the local device.
/// * `peer_addr` - The address of the peer device.
/// * `duration_ms` - Duration of the connection attempt in milliseconds.
/// * `params` - Optional connection parameters.
/// * `cb` - Callback function to handle events.
/// * `cb_arg` - Argument to pass to the callback function.
///
/// # Returns
///
/// Returns a `NimbleResult` indicating success or failure.
pub fn ble_gap_connect(
    own_addr_type: u8,
    peer_addr: &bindings::ble_addr_t,
    duration_ms: u32,
    params: Option<&bindings::ble_gap_conn_params>,
    cb: bindings::ble_gap_event_fn,
    cb_arg: *mut core::ffi::c_void,
) -> NimbleResult {
    let ret = unsafe {
        bindings::ble_gap_connect(
            own_addr_type,
            peer_addr,
            duration_ms as i32,
            match params {
                Some(params) => params,
                None => core::ptr::null(),
            },
            cb,
            cb_arg,
        )
    };
    return_code_to_result(ret as u32, ())
}

/// Terminates an active BLE GAP connection.
///
/// # Arguments
///
/// * `conn_handle` - The connection handle to terminate.
/// * `hci_reason` - The HCI reason code for termination.
///
/// # Returns
///
/// Returns a `NimbleResult` indicating success or failure.
pub fn ble_gap_terminate(conn_handle: ConnectionHandle, hci_reason: u8) -> NimbleResult {
    let ret = unsafe { bindings::ble_gap_terminate(conn_handle, hci_reason) };
    return_code_to_result(ret as u32, ())
}

/// Cancels an ongoing BLE GAP connection attempt.
///
/// # Returns
///
/// Returns a `NimbleResult` indicating success or failure.
pub fn ble_gap_conn_cancel() -> NimbleResult {
    let ret = unsafe { bindings::ble_gap_conn_cancel() };
    return_code_to_result(ret as u32, ())
}

/// Checks if a BLE GAP connection attempt is currently ongoing.
///
/// # Returns
///
/// Returns `true` if a connection is active, otherwise `false`.
pub fn ble_gap_conn_active() -> bool {
    unsafe { bindings::ble_gap_conn_active() != 0 }
}

/// Sets a callback for GAP events on a specific connection.
///
/// # Arguments
///
/// * `conn_handle` - The connection handle.
/// * `cb` - Callback function to handle events.
/// * `cb_arg` - Argument to pass to the callback function.
///
/// # Returns
///
/// Returns a `NimbleResult` indicating success or failure.
pub fn ble_gap_set_event_cb(
    conn_handle: ConnectionHandle,
    cb: bindings::ble_gap_event_fn,
    cb_arg: *mut core::ffi::c_void,
) -> NimbleResult {
    let ret = unsafe { bindings::ble_gap_set_event_cb(conn_handle, cb, cb_arg) };
    return_code_to_result(ret as u32, ())
}

/// Checks if BLE advertising is currently active.
///
/// # Returns
///
/// Returns `true` if advertising is active, otherwise `false`.
pub fn ble_gap_adv_active() -> bool {
    unsafe { bindings::ble_gap_adv_active() == 1 }
}

/// Starts BLE advertising with the given parameters.
///
/// # Arguments
///
/// * `own_addr_type` - The address type of the local device.
/// * `direct_addr` - Optional direct address for directed advertising.
/// * `duration_ms` - Duration of advertising in milliseconds.
/// * `adv_params` - Advertising parameters.
/// * `cb` - Callback function to handle events.
/// * `cb_arg` - Argument to pass to the callback function.
///
/// # Returns
///
/// Returns a `NimbleResult` indicating success or failure.
pub fn ble_gap_adv_start(
    own_addr_type: u8,
    direct_addr: Option<&bindings::ble_addr_t>,
    duration_ms: u32,
    adv_params: &bindings::ble_gap_adv_params,
    cb: bindings::ble_gap_event_fn,
    cb_arg: *mut core::ffi::c_void,
) -> NimbleResult {
    let ret = unsafe {
        bindings::ble_gap_adv_start(
            own_addr_type,
            direct_addr
                .map(|da| da as *const _)
                .unwrap_or(core::ptr::null_mut()),
            duration_ms as i32,
            adv_params,
            cb,
            cb_arg,
        )
    };
    return_code_to_result(ret as u32, ())
}

/// Stops ongoing BLE advertising.
///
/// # Returns
///
/// Returns a `NimbleResult` indicating success or failure.
pub fn ble_gap_adv_stop() -> NimbleResult {
    let ret = unsafe { bindings::ble_gap_adv_stop() };
    return_code_to_result(ret as u32, ())
}

/// Sets the raw advertising data for BLE advertising.
///
/// # Arguments
///
/// * `data` - The advertising data as a byte slice.
///
/// # Returns
///
/// Returns a `NimbleResult` indicating success or failure.
pub fn ble_gap_adv_set_data(data: &[u8]) -> NimbleResult {
    let ret = unsafe { bindings::ble_gap_adv_set_data(data.as_ptr(), data.len() as i32) };
    return_code_to_result(ret as u32, ())
}

/// Sets the advertising fields for BLE advertising.
///
/// # Arguments
///
/// * `rsp_fields` - The advertising fields to set.
///
/// # Returns
///
/// Returns a `NimbleResult` indicating success or failure.
pub fn ble_gap_adv_set_fields(rsp_fields: &bindings::ble_hs_adv_fields) -> NimbleResult {
    let ret = unsafe { bindings::ble_gap_adv_set_fields(rsp_fields) };
    return_code_to_result(ret as u32, ())
}

#[derive(Debug)]
pub enum BleGapEvent {
    Connect {
        status: u32,
        conn_handle: u16,
    },
    Disconnect {
        reason: u32,
        conn: bindings::ble_gap_conn_desc,
    },
    ConnUpdate {
        status: u32,
        conn_handle: u16,
    },
    ConnUpdateReq {
        peer_params: bindings::ble_gap_upd_params,
        self_params: bindings::ble_gap_upd_params,
        conn_handle: u16,
    },
    L2capUpdateReq,
    TermFailure,
    Disc,
    DiscComplete,
    AdvComplete,
    EncChange,
    PasskeyAction,
    NotifyRx,
    NotifyTx {
        status: u32,
        conn_handle: u16,
        attr_handle: u16,
    },
    Subscribe {
        conn_handle: u16,
        attr_handle: u16,
        reason: u8,
        prev_notify: bool,
        curr_notify: bool,
        prev_indicate: bool,
        curr_indicate: bool,
    },
    MTU {
        conn_handle: u16,
        channel_id: u16,
        value: u16,
    },
    IdentityResolved,
    RepeatPairing,
    PhyUpdateComplete {
        status: u32,
        conn_handle: u16,
        tx_phy: u8,
        rx_phy: u8,
    },
    ExtDisc,
    PeriodicSync,
    PeriodicReport,
    PeriodicSyncLost,
    ScanReqRcvd,
    PeriodicTransfer,
    PathlossThreshold,
    TransmitPower,
    SubrateChange,
    VsHci,
    ReattemptCount,
}

impl From<&bindings::ble_gap_event> for BleGapEvent {
    fn from(value: &bindings::ble_gap_event) -> Self {
        match value.type_ as u32 {
            bindings::BLE_GAP_EVENT_CONNECT => {
                let data = unsafe { value.__bindgen_anon_1.connect };
                BleGapEvent::Connect {
                    status: data.status as u32,
                    conn_handle: data.conn_handle,
                }
            }
            bindings::BLE_GAP_EVENT_DISCONNECT => {
                let data = unsafe { value.__bindgen_anon_1.disconnect };
                BleGapEvent::Disconnect {
                    reason: data.reason as u32,
                    conn: data.conn,
                }
            }
            bindings::BLE_GAP_EVENT_CONN_UPDATE => {
                let data = unsafe { value.__bindgen_anon_1.conn_update };
                BleGapEvent::ConnUpdate {
                    status: data.status as u32,
                    conn_handle: data.conn_handle,
                }
            }
            bindings::BLE_GAP_EVENT_CONN_UPDATE_REQ => {
                let data = unsafe { value.__bindgen_anon_1.conn_update_req };
                BleGapEvent::ConnUpdateReq {
                    conn_handle: data.conn_handle,
                    peer_params: unsafe { *data.peer_params },
                    self_params: unsafe { *data.self_params },
                }
            }
            bindings::BLE_GAP_EVENT_PHY_UPDATE_COMPLETE => {
                let data = unsafe { value.__bindgen_anon_1.phy_updated };

                BleGapEvent::PhyUpdateComplete {
                    status: data.status as u32,
                    conn_handle: data.conn_handle,
                    tx_phy: data.tx_phy,
                    rx_phy: data.rx_phy,
                }
            }
            bindings::BLE_GAP_EVENT_NOTIFY_TX => {
                let data = unsafe { value.__bindgen_anon_1.notify_tx };

                BleGapEvent::NotifyTx {
                    status: data.status as u32,
                    conn_handle: data.conn_handle,
                    attr_handle: data.attr_handle,
                }
            }
            bindings::BLE_GAP_EVENT_SUBSCRIBE => {
                let data = unsafe { value.__bindgen_anon_1.subscribe };

                BleGapEvent::Subscribe {
                    conn_handle: data.conn_handle,
                    attr_handle: data.attr_handle,
                    reason: data.reason,
                    prev_notify: data.prev_notify() == 1,
                    curr_notify: data.cur_notify() == 1,
                    prev_indicate: data.prev_indicate() == 1,
                    curr_indicate: data.cur_indicate() == 1,
                }
            }
            bindings::BLE_GAP_EVENT_MTU => {
                let data = unsafe { value.__bindgen_anon_1.mtu };

                BleGapEvent::MTU {
                    conn_handle: data.conn_handle,
                    channel_id: data.channel_id,
                    value: data.value,
                }
            }
            _ => panic!("Unknown GAP event type: {}", value.type_),
        }
    }
}
