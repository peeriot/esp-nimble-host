use alloc::{boxed::Box, sync::Arc, vec::Vec};

use bytes::Bytes;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
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
#[derive(Clone, Debug)]
pub struct Characteristic {
    uuid: Uuid,
    handle: u16,
    def_handle: u16,
    descriptors: Vec<Descriptor>,
}

impl Characteristic {
    pub(crate) fn new(uuid: Uuid, handle: u16, def_handle: u16) -> Self {
        Self {
            uuid,
            handle,
            def_handle,
            descriptors: Vec::new(),
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

    pub fn descriptors(&self) -> &[Descriptor] {
        &self.descriptors
    }

    pub fn descriptors_mut(&mut self) -> &mut Vec<Descriptor> {
        &mut self.descriptors
    }
}

/// Represents the context for a read operation.
pub type ReadOperationContext = Option<Bytes>;

/// Reads a BLE attribute.
pub async fn read_attribute(conn_handle: ConnectionHandle, handle: u16) -> GattResult<Bytes> {
    let (op_box, op_handle) =
        peripheral_operation::<ReadOperationContext, CriticalSectionRawMutex>(conn_handle, None);
    let op_ptr = Box::into_raw(op_box);

    if let Err(e) = ble_gattc_read(
        conn_handle,
        handle,
        Some(read_attribute_callback::<CriticalSectionRawMutex>),
        op_ptr as _,
    ) {
        drop(unsafe { Box::from_raw(op_ptr) });
        return Err(GattError::ReadFailed(e));
    }

    op_handle.join().await?;
    op_handle.take_context().flatten().ok_or(GattError::NoData)
}

extern "C" fn read_attribute_callback<M>(
    conn_handle: ConnectionHandle,
    error: *const ble_gatt_error,
    attr: *mut ble_gatt_attr,
    arg: *mut core::ffi::c_void,
) -> i32
where
    M: embassy_sync::blocking_mutex::raw::RawMutex,
{
    if arg.is_null() || error.is_null() {
        log::error!(
            "read_attribute_callback received null pointer: arg={arg:p}, error={error:p}"
        );
        return -1;
    }

    let error = unsafe { &*error };

    let is_ours = {
        let op = unsafe { &*(arg as *const PeripheralOperation<ReadOperationContext, M>) };
        conn_handle == op.conn_handle()
    };
    if !is_ours {
        return 0;
    }

    // All read callbacks are terminal — reconstitute and drop the Box.
    let op_box =
        unsafe { Box::from_raw(arg as *mut PeripheralOperation<ReadOperationContext, M>) };

    if error.status != 0 {
        op_box.send_finished(
            return_code_to_result(error.status as u32, ()).map_err(GattError::ReadFailed),
        );
        return 0;
    }

    if attr.is_null() {
        log::error!("read_attribute_callback: status==0 but attr is NULL");
        op_box.send_finished(Err(GattError::NoData));
        return 0;
    }

    let attr = unsafe { &*attr };

    match ble_hs_mbuf_to_flat(attr.om) {
        Ok(om_data) => {
            op_box
                .context()
                .lock(|ctx| *ctx.borrow_mut() = Some(Bytes::from(om_data)));
            op_box.send_finished(Ok(()));
        }
        Err(e) => {
            op_box.send_finished(Err(GattError::ReadFailed(e)));
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
    let data = data.as_ref();

    let mtu = ble_att_mtu(conn_handle).map_err(GattError::WriteFailed)?;
    let mtu = (mtu
        .get()
        .checked_sub(3)
        .ok_or(GattError::AttMtuZero(conn_handle))?) as usize;

    if !response && data.len() <= mtu {
        return ble_gattc_write_no_rsp_flat(conn_handle, attr_handle, data)
            .map_err(GattError::WriteFailed);
    }

    let (op_box, op_handle) =
        peripheral_operation::<(), CriticalSectionRawMutex>(conn_handle, ());
    let op_ptr = Box::into_raw(op_box);

    let write_result = if data.len() <= mtu {
        ble_gattc_write_flat(
            conn_handle,
            attr_handle,
            data,
            Some(write_attribute_callback::<CriticalSectionRawMutex>),
            op_ptr as _,
        )
    } else {
        ble_gattc_write_long(
            conn_handle,
            attr_handle,
            0,
            data,
            Some(write_attribute_callback::<CriticalSectionRawMutex>),
            op_ptr as _,
        )
    };

    if let Err(e) = write_result {
        drop(unsafe { Box::from_raw(op_ptr) });
        return Err(GattError::WriteFailed(e));
    }

    op_handle.join().await
}

extern "C" fn write_attribute_callback<M>(
    conn_handle: ConnectionHandle,
    error: *const ble_gatt_error,
    _attr: *mut ble_gatt_attr,
    arg: *mut core::ffi::c_void,
) -> i32
where
    M: embassy_sync::blocking_mutex::raw::RawMutex,
{
    if arg.is_null() || error.is_null() {
        log::error!("write_attribute_callback received null pointer");
        return -1;
    }

    let error = unsafe { &*error };

    let is_ours = {
        let op = unsafe { &*(arg as *const PeripheralOperation<(), M>) };
        conn_handle == op.conn_handle()
    };
    if !is_ours {
        return 0;
    }

    // All write callbacks are terminal — reconstitute and drop the Box.
    let op_box = unsafe { Box::from_raw(arg as *mut PeripheralOperation<(), M>) };
    op_box.send_finished(
        return_code_to_result(error.status as u32, ()).map_err(GattError::WriteFailed),
    );

    error.status as _
}
