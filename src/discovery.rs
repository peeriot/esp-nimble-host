use alloc::{boxed::Box, vec::Vec};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use uuid::Uuid;

use crate::{
    characteristic::{Characteristic, Descriptor},
    data::{ConnectionHandle, nimble_uuid_to_uuid, uuid_to_nimble_uuid},
    error::{GattError, GattResult},
    nimble_sys::{
        NimbleError,
        bindings::{BLE_HS_EDONE, ble_gatt_chr, ble_gatt_dsc, ble_gatt_error, ble_gatt_svc},
        ble_gattc_disc_all_chrs, ble_gattc_disc_all_dscs, ble_gattc_disc_all_svcs,
        ble_gattc_disc_svc_by_uuid, return_code_to_result,
    },
    peripheral_operation::{PeripheralOperation, peripheral_operation},
    service::Service,
};

/// Discovers all services, characteristics, and descriptors on a connection.
#[derive(Clone)]
pub(crate) struct ServiceDiscovery {
    conn_handle: ConnectionHandle,
}

impl ServiceDiscovery {
    pub fn new(conn_handle: ConnectionHandle) -> Self {
        Self { conn_handle }
    }

    pub async fn run(self) -> GattResult<Vec<Service>> {
        let mut services = nimble_discover_services(self.conn_handle).await?;

        for service in &mut services {
            let chars = nimble_discover_characteristics(
                self.conn_handle,
                service.start_handle(),
                service.end_handle(),
            )
            .await?;

            *service.characteristics_mut() = chars;

            let svc_end = service.end_handle();
            let char_count = service.characteristics().len();
            for i in 0..char_count {
                // Descriptor range: after the characteristic value handle,
                // up to the next characteristic's definition handle - 1
                // (or the service end handle for the last characteristic).
                // NimBLE searches from start_handle + 1, so start must be < end.
                let chr_val_handle = service.characteristics()[i].handle();
                let desc_end = if i + 1 < char_count {
                    service.characteristics()[i + 1].def_handle() - 1
                } else {
                    svc_end
                };

                // Skip if no handle space for descriptors
                if chr_val_handle >= desc_end {
                    continue;
                }

                let descriptors = nimble_discover_characteristic_descriptors(
                    self.conn_handle,
                    chr_val_handle,
                    desc_end,
                )
                .await?;

                *service.characteristics_mut()[i].descriptors_mut() = descriptors;
            }
        }

        Ok(services)
    }
}

/// Discovers a specific service by UUID, including its characteristics and descriptors.
#[derive(Clone)]
pub(crate) struct ServiceCharacteristicsDiscovery {
    conn_handle: ConnectionHandle,
    service_uuid: Uuid,
}

impl ServiceCharacteristicsDiscovery {
    pub fn new(conn_handle: ConnectionHandle, service_uuid: &Uuid) -> Self {
        Self {
            conn_handle,
            service_uuid: *service_uuid,
        }
    }

    pub async fn run(self) -> GattResult<Option<Service>> {
        let mut services =
            nimble_discover_service_by_uuid(self.conn_handle, &self.service_uuid).await?;

        let Some(mut service) = services.pop() else {
            return Ok(None);
        };

        let chars = nimble_discover_characteristics(
            self.conn_handle,
            service.start_handle(),
            service.end_handle(),
        )
        .await?;

        *service.characteristics_mut() = chars;

        let svc_end = service.end_handle();
        let char_count = service.characteristics().len();
        for i in 0..char_count {
            let chr_val_handle = service.characteristics()[i].handle();
            let desc_end = if i + 1 < char_count {
                service.characteristics()[i + 1].def_handle() - 1
            } else {
                svc_end
            };

            if chr_val_handle >= desc_end {
                continue;
            }

            let descriptors = nimble_discover_characteristic_descriptors(
                self.conn_handle,
                chr_val_handle,
                desc_end,
            )
            .await?;

            *service.characteristics_mut()[i].descriptors_mut() = descriptors;
        }

        Ok(Some(service))
    }
}

// ── NimBLE GATT discovery primitives ─────────────────────────────────────────

/// Converts a terminal discovery callback status into a `GattResult`.
///
/// `BLE_HS_EDONE` means all items were delivered (success); any other non-zero
/// status is a genuine error wrapped via `err_map`.
fn disc_terminal_result(status: i32, err_map: fn(NimbleError) -> GattError) -> GattResult {
    if status == BLE_HS_EDONE as i32 {
        return Ok(());
    }
    Err(err_map(return_code_to_result(status as u32, ()).unwrap_err()))
}

async fn nimble_discover_services(conn_handle: ConnectionHandle) -> GattResult<Vec<Service>> {
    let (op_box, handle) =
        peripheral_operation::<Vec<Service>, CriticalSectionRawMutex>(conn_handle, Vec::new());
    let op_ptr = Box::into_raw(op_box);

    if let Err(e) = ble_gattc_disc_all_svcs(
        conn_handle,
        Some(service_discovered_cb::<CriticalSectionRawMutex>),
        op_ptr as _,
    ) {
        drop(unsafe { Box::from_raw(op_ptr) });
        return Err(GattError::ServiceDiscoveryFailed(e));
    }

    handle.join().await?;
    handle.take_context().ok_or(GattError::NoServicesDiscovered)
}

async fn nimble_discover_service_by_uuid(
    conn_handle: ConnectionHandle,
    service_uuid: &Uuid,
) -> GattResult<Vec<Service>> {
    let (op_box, handle) =
        peripheral_operation::<Vec<Service>, CriticalSectionRawMutex>(conn_handle, Vec::new());
    let op_ptr = Box::into_raw(op_box);

    if let Err(e) = ble_gattc_disc_svc_by_uuid(
        conn_handle,
        &uuid_to_nimble_uuid(service_uuid),
        Some(service_discovered_cb::<CriticalSectionRawMutex>),
        op_ptr as _,
    ) {
        drop(unsafe { Box::from_raw(op_ptr) });
        return Err(GattError::ServiceDiscoveryFailed(e));
    }

    handle.join().await?;
    handle.take_context().ok_or(GattError::NoServicesDiscovered)
}

extern "C" fn service_discovered_cb<M>(
    conn_handle: ConnectionHandle,
    error: *const ble_gatt_error,
    service: *const ble_gatt_svc,
    arg: *mut core::ffi::c_void,
) -> i32
where
    M: embassy_sync::blocking_mutex::raw::RawMutex,
{
    if error.is_null() || arg.is_null() {
        log::error!("service_discovered_cb received null pointer for arg/error");
        return -1;
    }

    let error = unsafe { &*error };

    let is_ours = {
        let op = unsafe { &*(arg as *const PeripheralOperation<Vec<Service>, M>) };
        conn_handle == op.conn_handle()
    };
    if !is_ours {
        return 0;
    }

    if error.status == 0 {
        // Non-terminal: accumulate service into context.
        if service.is_null() {
            log::error!("service_discovered_cb received null pointer for service");
            return -1;
        }
        let op = unsafe { &*(arg as *const PeripheralOperation<Vec<Service>, M>) };
        let service = unsafe { &*service };
        match nimble_uuid_to_uuid(&service.uuid) {
            Ok(uuid) => {
                op.context().lock(|v| {
                    v.borrow_mut().push(Service::new(uuid, service.start_handle, service.end_handle))
                });
            }
            Err(_) => {
                // Option A: return non-zero so NimBLE cancels and re-enters with an error status.
                log::warn!("service_discovered_cb: failed to parse UUID");
                return -1;
            }
        }
        return 0;
    }

    // Terminal call (EDONE or genuine error) — reconstitute and drop the Box.
    let op_box = unsafe { Box::from_raw(arg as *mut PeripheralOperation<Vec<Service>, M>) };
    op_box.send_finished(disc_terminal_result(
        error.status as i32,
        GattError::ServiceDiscoveryFailed,
    ));
    0
}

async fn nimble_discover_characteristics(
    conn_handle: ConnectionHandle,
    start_handle: u16,
    end_handle: u16,
) -> GattResult<Vec<Characteristic>> {
    let (op_box, handle) = peripheral_operation::<Vec<Characteristic>, CriticalSectionRawMutex>(
        conn_handle,
        Vec::new(),
    );
    let op_ptr = Box::into_raw(op_box);

    if let Err(e) = ble_gattc_disc_all_chrs(
        conn_handle,
        start_handle,
        end_handle,
        Some(characteristic_disc_cb::<CriticalSectionRawMutex>),
        op_ptr as _,
    ) {
        drop(unsafe { Box::from_raw(op_ptr) });
        return Err(GattError::CharacteristicDiscoveryFailed(e));
    }

    handle.join().await?;
    handle
        .take_context()
        .ok_or(GattError::NoCharacteristicsDiscovered)
}

extern "C" fn characteristic_disc_cb<M>(
    conn_handle: ConnectionHandle,
    error: *const ble_gatt_error,
    chr: *const ble_gatt_chr,
    arg: *mut core::ffi::c_void,
) -> i32
where
    M: embassy_sync::blocking_mutex::raw::RawMutex,
{
    if error.is_null() || arg.is_null() {
        log::error!("characteristic_disc_cb received null pointer");
        return -1;
    }

    let error = unsafe { &*error };

    let is_ours = {
        let op = unsafe { &*(arg as *const PeripheralOperation<Vec<Characteristic>, M>) };
        conn_handle == op.conn_handle()
    };
    if !is_ours {
        return 0;
    }

    if error.status == 0 {
        // Non-terminal: accumulate characteristic into context.
        if chr.is_null() {
            log::error!("characteristic_disc_cb received null pointer for chr");
            return -1;
        }
        let op = unsafe { &*(arg as *const PeripheralOperation<Vec<Characteristic>, M>) };
        let chr = unsafe { &*chr };
        match nimble_uuid_to_uuid(&chr.uuid) {
            Ok(uuid) => {
                op.context().lock(|v| {
                    v.borrow_mut()
                        .push(Characteristic::new(uuid, chr.val_handle, chr.def_handle));
                });
            }
            Err(_) => {
                log::warn!("characteristic_disc_cb: failed to parse UUID");
                return -1;
            }
        }
        return 0;
    }

    // Terminal call — reconstitute and drop the Box.
    let op_box =
        unsafe { Box::from_raw(arg as *mut PeripheralOperation<Vec<Characteristic>, M>) };
    op_box.send_finished(disc_terminal_result(
        error.status as i32,
        GattError::CharacteristicDiscoveryFailed,
    ));
    0
}

async fn nimble_discover_characteristic_descriptors(
    conn_handle: ConnectionHandle,
    start_handle: u16,
    end_handle: u16,
) -> GattResult<Vec<Descriptor>> {
    let (op_box, handle) =
        peripheral_operation::<Vec<Descriptor>, CriticalSectionRawMutex>(conn_handle, Vec::new());
    let op_ptr = Box::into_raw(op_box);

    if let Err(e) = ble_gattc_disc_all_dscs(
        conn_handle,
        start_handle,
        end_handle,
        Some(characteristic_descriptor_disc_cb::<CriticalSectionRawMutex>),
        op_ptr as _,
    ) {
        drop(unsafe { Box::from_raw(op_ptr) });
        return Err(GattError::DescriptorDiscoveryFailed(e));
    }

    handle.join().await?;
    handle
        .take_context()
        .ok_or(GattError::NoDescriptorsDiscovered)
}

extern "C" fn characteristic_descriptor_disc_cb<M>(
    conn_handle: ConnectionHandle,
    error: *const ble_gatt_error,
    _char_val_handle: u16,
    dsc: *const ble_gatt_dsc,
    arg: *mut core::ffi::c_void,
) -> i32
where
    M: embassy_sync::blocking_mutex::raw::RawMutex,
{
    if error.is_null() || arg.is_null() {
        log::error!("characteristic_descriptor_disc_cb received null pointer");
        return -1;
    }

    let error = unsafe { &*error };

    let is_ours = {
        let op = unsafe { &*(arg as *const PeripheralOperation<Vec<Descriptor>, M>) };
        conn_handle == op.conn_handle()
    };
    if !is_ours {
        return 0;
    }

    if error.status == 0 {
        // Non-terminal: accumulate descriptor into context.
        if dsc.is_null() {
            log::error!("characteristic_descriptor_disc_cb received null pointer for dsc");
            return -1;
        }
        let op = unsafe { &*(arg as *const PeripheralOperation<Vec<Descriptor>, M>) };
        let dsc = unsafe { &*dsc };
        match nimble_uuid_to_uuid(&dsc.uuid) {
            Ok(uuid) => {
                op.context().lock(|v| {
                    v.borrow_mut().push(Descriptor::new(uuid, dsc.handle));
                });
            }
            Err(_) => {
                log::warn!("characteristic_descriptor_disc_cb: failed to parse UUID");
                return -1;
            }
        }
        return 0;
    }

    // Terminal call — reconstitute and drop the Box.
    let op_box = unsafe { Box::from_raw(arg as *mut PeripheralOperation<Vec<Descriptor>, M>) };
    op_box.send_finished(disc_terminal_result(
        error.status as i32,
        GattError::DescriptorDiscoveryFailed,
    ));
    0
}
