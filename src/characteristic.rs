use alloc::{collections::BTreeSet, sync::Arc};

use bytes::Bytes;
use embassy_sync::blocking_mutex::raw::NoopRawMutex;
use uuid::Uuid;

use crate::{
    data::{AttributeHandle, ConnectionHandle},
    error::{GattError, GattResult},
    nimble_sys::{
        bindings::{ble_gatt_attr, ble_gatt_error},
        ble_att_mtu, ble_gattc_read, ble_gattc_write_flat, ble_gattc_write_long,
        ble_gattc_write_no_rsp_flat, ble_hs_mbuf_to_flat, return_code_to_result,
    },
    peripheral_operation::{PeripheralOperation, peripheral_operation},
};

/// Represents a BLE descriptor with a UUID and handle.
#[derive(Clone, Debug, PartialOrd, Ord, PartialEq, Eq)]
pub struct Descriptor {
    uuid: Uuid,
    handle: u16,
}

impl Descriptor {
    pub(crate) fn new(uuid: Uuid, handle: u16) -> Self {
        Self { uuid, handle }
    }

    pub fn handle(&self) -> u16 {
        self.handle
    }

    pub fn uuid(&self) -> Uuid {
        self.uuid
    }
}

/// Represents a BLE characteristic with a UUID, handle, and associated descriptors.
#[derive(Clone, Debug, PartialOrd, Ord, PartialEq, Eq)]
pub struct Characteristic {
    uuid: Uuid,
    handle: u16,
    def_handle: u16,
    descriptors: BTreeSet<Descriptor>,
}

impl Characteristic {
    pub(crate) fn new(uuid: Uuid, handle: u16, def_handle: u16) -> Self {
        Self {
            uuid,
            handle,
            def_handle,
            descriptors: Default::default(),
        }
    }

    pub fn handle(&self) -> u16 {
        self.handle
    }

    pub fn def_handle(&self) -> u16 {
        self.def_handle
    }

    pub fn uuid(&self) -> Uuid {
        self.uuid
    }

    pub fn descriptors(&self) -> &BTreeSet<Descriptor> {
        &self.descriptors
    }

    pub fn descriptors_mut(&mut self) -> &mut BTreeSet<Descriptor> {
        &mut self.descriptors
    }
}

/// Represents the context for a read operation.
pub type ReadOperationContext = Option<Bytes>;

/// Reads a BLE attribute.
pub async fn read_attribute(conn_handle: ConnectionHandle, handle: u16) -> GattResult<Bytes> {
    // If your helper is generic over RawMutex, pass it here explicitly (e.g. <ReadOperationContext, M>).
    let (operation, operation_handle) =
        peripheral_operation::<ReadOperationContext, NoopRawMutex>(conn_handle, None);

    ble_gattc_read(
        conn_handle,
        handle,
        Some(read_attribute_callback::<NoopRawMutex>),
        &operation as *const PeripheralOperation<ReadOperationContext, _> as _,
    )
    .map_err(GattError::ReadFailed)?;

    operation_handle.join().await?;

    operation
        .take_context()
        .flatten()
        .ok_or(GattError::NoData)
}

extern "C" fn read_attribute_callback<M>(
    conn_handle: ConnectionHandle,
    error: *const ble_gatt_error,
    attr: *mut ble_gatt_attr,
    operation: *mut core::ffi::c_void,
) -> i32
where
    M: embassy_sync::blocking_mutex::raw::RawMutex,
{
    if operation.is_null() || error.is_null() {
        log::error!(
            "read_attribute_callback received null pointer: operation={operation:p}, error={error:p}"
        );
        return -1;
    }

    let operation = unsafe { &*(operation as *const PeripheralOperation<ReadOperationContext, M>) };
    let error = unsafe { &*error };

    if conn_handle != operation.conn_handle() {
        return 0;
    }

    // If status is nonzero, attr may legitimately be null.
    if error.status != 0 {
        operation.send_finished(
            return_code_to_result(error.status as u32, ()).map_err(GattError::ReadFailed),
        );
        return 0;
    }

    // status == 0 => we must have an attr with data
    if attr.is_null() {
        log::error!("read_attribute_callback: status==0 but attr is NULL");
        operation.send_finished(Err(GattError::NoData));
        return 0;
    }

    let attr = unsafe { &*attr };

    match ble_hs_mbuf_to_flat(attr.om) {
        Ok(om_data) => {
            unsafe {
                operation
                    .context()
                    .lock_mut(|ctx| *ctx = Some(Bytes::from(om_data)))
            };
            operation.send_finished(Ok(()));
        }
        Err(e) => {
            operation.send_finished(Err(GattError::ReadFailed(e)));
        }
    }

    0
}

/// Writes a BLE attribute.
pub async fn write_attribute(
    conn_handle: ConnectionHandle,
    attr_handle: AttributeHandle,
    data: Arc<[u8]>,
    response: bool,
) -> GattResult {
    let (operation, operation_handle) = peripheral_operation::<(), NoopRawMutex>(conn_handle, ());
    let data = data.as_ref();

    let mtu = ble_att_mtu(conn_handle).map_err(GattError::WriteFailed)?;

    let mtu = (mtu
        .get()
        .checked_sub(3)
        .ok_or(GattError::AttMtuZero(conn_handle))?)
        as usize;

    if !response && data.len() <= mtu {
        ble_gattc_write_no_rsp_flat(conn_handle, attr_handle, data)
            .map_err(GattError::WriteFailed)?;
        return Ok(());
    }

    if data.len() <= mtu {
        ble_gattc_write_flat(
            conn_handle,
            attr_handle,
            data,
            Some(write_attribute_callback::<NoopRawMutex>),
            &operation as *const PeripheralOperation<(), _> as _,
        )
        .map_err(GattError::WriteFailed)?;
    } else {
        ble_gattc_write_long(
            conn_handle,
            attr_handle,
            0,
            data,
            Some(write_attribute_callback::<NoopRawMutex>),
            &operation as *const PeripheralOperation<(), _> as _,
        )
        .map_err(GattError::WriteFailed)?;
    }

    operation_handle.join().await
}

extern "C" fn write_attribute_callback<M>(
    conn_handle: ConnectionHandle,
    error: *const ble_gatt_error,
    _attr: *mut ble_gatt_attr,
    operation: *mut core::ffi::c_void,
) -> i32
where
    M: embassy_sync::blocking_mutex::raw::RawMutex,
{
    if operation.is_null() || error.is_null() {
        log::error!("write_attribute_callback received null pointer");
        return -1;
    }

    let operation = unsafe { &*(operation as *const PeripheralOperation<(), M>) };
    let error = unsafe { &*error };

    if conn_handle != operation.conn_handle() {
        return 0;
    }

    operation.send_finished(
        return_code_to_result(error.status as u32, ()).map_err(GattError::WriteFailed),
    );

    error.status as _
}
