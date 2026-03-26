use alloc::{collections::BTreeSet, vec::Vec};
use embassy_sync::blocking_mutex::raw::NoopRawMutex;
use uuid::Uuid;

use crate::{
    characteristic::{Characteristic, Descriptor},
    data::{ConnectionHandle, nimble_uuid_to_uuid, uuid_to_nimble_uuid},
    error::{GattError, GattResult},
    nimble_sys::{
        bindings::{BLE_HS_EDONE, ble_gatt_chr, ble_gatt_dsc, ble_gatt_error, ble_gatt_svc},
        ble_gattc_disc_all_chrs, ble_gattc_disc_all_dscs, ble_gattc_disc_all_svcs,
        ble_gattc_disc_svc_by_uuid,
    },
    peripheral_operation::{PeripheralOperation, peripheral_operation},
    service::Service,
};

/// Struct for discovering services, characteristics, and descriptors in a BLE connection.
#[derive(Clone)]
pub(crate) struct ServiceDiscovery {
    conn_handle: ConnectionHandle,
}

impl ServiceDiscovery {
    pub fn new(conn_handle: ConnectionHandle) -> Self {
        Self { conn_handle }
    }

    pub async fn run(self) -> GattResult<BTreeSet<Service>> {
        let mut services = nimble_discover_services(self.conn_handle).await?;

        for service in services.iter_mut() {
            self.discover_characteristics(service).await?;

            let mut chars_vec: Vec<_> = service.characteristics().clone().into_iter().collect();
            for ch in chars_vec.iter_mut() {
                self.discover_descriptors(service, ch).await?;
            }

            *service.characteristics_mut() = BTreeSet::from_iter(chars_vec);
        }

        Ok(services.into_iter().collect())
    }

    async fn discover_characteristics(&self, service: &mut Service) -> GattResult<()> {
        let chars = nimble_discover_characteristics(
            self.conn_handle,
            service.start_handle(),
            service.end_handle(),
        )
        .await?;

        *service.characteristics_mut() = chars;
        Ok(())
    }

    async fn discover_descriptors(
        &self,
        service: &Service,
        characteristic: &mut Characteristic,
    ) -> GattResult<()> {
        // NOTE: your original code passed service start/end for descriptor discovery.
        // If you intended "per characteristic", you likely want (chr_def_handle..end_handle)
        // or (chr_val_handle..service_end). Kept same semantics as your snippet.
        let descriptors = nimble_discover_characteristic_descriptors(
            self.conn_handle,
            characteristic.def_handle(),
            service.end_handle(),
        )
        .await?;

        *characteristic.descriptors_mut() = descriptors;
        Ok(())
    }
}

/// Struct for discovering characteristics of a specific service.
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

        if let Some(mut service) = services.pop() {
            self.discover_characteristics(&mut service).await?;

            let mut chars_vec: Vec<_> = service.characteristics().clone().into_iter().collect();
            for ch in chars_vec.iter_mut() {
                self.discover_descriptors(&mut service, ch).await?;
            }

            *service.characteristics_mut() = BTreeSet::from_iter(chars_vec);

            return Ok(Some(service));
        }

        Ok(None)
    }

    async fn discover_characteristics(&self, service: &mut Service) -> GattResult<()> {
        let chars = nimble_discover_characteristics(
            self.conn_handle,
            service.start_handle(),
            service.end_handle(),
        )
        .await?;

        *service.characteristics_mut() = chars;
        Ok(())
    }

    async fn discover_descriptors(
        &self,
        service: &Service,
        characteristic: &mut Characteristic,
    ) -> GattResult<()> {
        // NOTE: your original code passed service start/end for descriptor discovery.
        // If you intended "per characteristic", you likely want (chr_def_handle..end_handle)
        // or (chr_val_handle..service_end). Kept same semantics as your snippet.
        log::trace!("Discovering descriptors...");
        let descriptors = nimble_discover_characteristic_descriptors(
            self.conn_handle,
            characteristic.def_handle(),
            service.end_handle(),
        )
        .await?;

        log::trace!("Descriptors: {descriptors:?}");

        *characteristic.descriptors_mut() = descriptors;
        Ok(())
    }
}

/// Initiates the discovery of all services.
async fn nimble_discover_services(conn_handle: ConnectionHandle) -> GattResult<Vec<Service>> {
    // IMPORTANT: choose the same RawMutex you used elsewhere (e.g. CriticalSectionRawMutex).
    // If your helper is generic like peripheral_operation::<T, M>, pass the M.
    let (operation, operation_handle) =
        peripheral_operation::<Vec<Service>, NoopRawMutex>(conn_handle, Vec::new());

    ble_gattc_disc_all_svcs(
        conn_handle,
        Some(service_discovered_cb::<NoopRawMutex>),
        &operation as *const PeripheralOperation<Vec<Service>, _> as _,
    )
    .map_err(GattError::ServiceDiscoveryFailed)?;

    operation_handle.join().await?;
    operation
        .take_context()
        .ok_or_else(|| GattError::NoServicesDiscovered)
}

/// Initiates the discovery of services by uuid.
async fn nimble_discover_service_by_uuid(
    conn_handle: ConnectionHandle,
    service_uuid: &Uuid,
) -> GattResult<Vec<Service>> {
    let (operation, operation_handle) =
        peripheral_operation::<Vec<Service>, NoopRawMutex>(conn_handle, Vec::new());

    ble_gattc_disc_svc_by_uuid(
        conn_handle,
        &uuid_to_nimble_uuid(service_uuid),
        Some(service_discovered_cb::<NoopRawMutex>),
        &operation as *const PeripheralOperation<Vec<Service>, _> as _,
    )
    .map_err(GattError::ServiceDiscoveryFailed)?;

    operation_handle.join().await?;
    operation
        .take_context()
        .ok_or_else(|| GattError::NoServicesDiscovered)
}

/// Callback: service discovery
extern "C" fn service_discovered_cb<M>(
    conn_handle: ConnectionHandle,
    error: *const ble_gatt_error,
    service: *const ble_gatt_svc,
    operation: *mut core::ffi::c_void,
) -> i32
where
    // Match the PeripheralOperation you ported: context is embassy blocking mutex.
    M: embassy_sync::blocking_mutex::raw::RawMutex,
{
    if error.is_null() || operation.is_null() {
        log::error!("service_discovered_cb received null pointer for operation/error");
        return -1;
    }

    let operation = unsafe { &*(operation as *const PeripheralOperation<Vec<Service>, M>) };
    if conn_handle != operation.conn_handle() {
        return 0;
    }

    let error = unsafe { &*error };

    if error.status == 0 {
        if service.is_null() {
            log::error!("service_discovered_cb received null pointer for service");
            return -1;
        }

        let service = unsafe { &*service };
        match nimble_uuid_to_uuid(&service.uuid) {
            Ok(uuid) => {
                unsafe {
                    operation.context().lock_mut(|v| {
                        v.push(Service::new(uuid, service.start_handle, service.end_handle))
                    })
                };
            }
            Err(e) => {
                operation.send_finished(Err(e.into()));
            }
        }
        return 0;
    }

    if error.status == (BLE_HS_EDONE as _) {
        operation.send_finished(Ok(()));
        return 0;
    }

    error.status as _
}

/// Initiates the discovery of characteristics within a specified handle range.
async fn nimble_discover_characteristics(
    conn_handle: ConnectionHandle,
    start_handle: u16,
    end_handle: u16,
) -> GattResult<BTreeSet<Characteristic>> {
    let (operation, operation_handle) = peripheral_operation::<
        BTreeSet<Characteristic>,
        NoopRawMutex,
    >(conn_handle, BTreeSet::new());

    ble_gattc_disc_all_chrs(
        conn_handle,
        start_handle,
        end_handle,
        Some(characteristic_disc_cb::<NoopRawMutex>),
        &operation as *const PeripheralOperation<BTreeSet<Characteristic>, _> as _,
    )
    .map_err(GattError::CharacteristicDiscoveryFailed)?;

    operation_handle.join().await?;
    operation
        .take_context()
        .ok_or_else(|| GattError::NoCharacteristicsDiscovered)
}

extern "C" fn characteristic_disc_cb<M>(
    conn_handle: ConnectionHandle,
    error: *const ble_gatt_error,
    chr: *const ble_gatt_chr,
    operation: *mut core::ffi::c_void,
) -> i32
where
    M: embassy_sync::blocking_mutex::raw::RawMutex,
{
    if error.is_null() || operation.is_null() {
        log::error!("characteristic_disc_cb received null pointer");
        return -1;
    }

    let operation =
        unsafe { &*(operation as *const PeripheralOperation<BTreeSet<Characteristic>, M>) };
    if conn_handle != operation.conn_handle() {
        return 0;
    }

    let error = unsafe { &*error };

    if error.status == 0 {
        if chr.is_null() {
            log::error!("characteristic_disc_cb received null pointer for chr");
            return -1;
        }

        let chr = unsafe { &*chr };
        match nimble_uuid_to_uuid(&chr.uuid) {
            Ok(uuid) => {
                unsafe {
                    operation.context().lock_mut(|set| {
                        set.insert(Characteristic::new(uuid, chr.val_handle, chr.def_handle));
                    })
                };
            }
            Err(e) => {
                operation.send_finished(Err(e.into()));
            }
        }
        return 0;
    }

    // In NimBLE, "done" is typically BLE_HS_EDONE; your original code finished on any non-0.
    // Keeping original behavior:
    operation.send_finished(Ok(()));
    error.status as _
}

/// Initiates the discovery of descriptors within a specified handle range.
async fn nimble_discover_characteristic_descriptors(
    conn_handle: ConnectionHandle,
    start_handle: u16,
    end_handle: u16,
) -> GattResult<BTreeSet<Descriptor>> {
    let (operation, operation_handle) =
        peripheral_operation::<BTreeSet<Descriptor>, NoopRawMutex>(conn_handle, BTreeSet::new());

    ble_gattc_disc_all_dscs(
        conn_handle,
        start_handle,
        end_handle,
        Some(characteristic_descriptor_disc_cb::<NoopRawMutex>),
        &operation as *const PeripheralOperation<BTreeSet<Descriptor>, _> as _,
    )
    .map_err(GattError::DescriptorDiscoveryFailed)?;

    operation_handle.join().await?;
    operation
        .take_context()
        .ok_or_else(|| GattError::NoDescriptorsDiscovered)
}

extern "C" fn characteristic_descriptor_disc_cb<M>(
    conn_handle: ConnectionHandle,
    error: *const ble_gatt_error,
    _char_val_handle: u16,
    dsc: *const ble_gatt_dsc,
    operation: *mut core::ffi::c_void,
) -> i32
where
    M: embassy_sync::blocking_mutex::raw::RawMutex,
{
    if error.is_null() || operation.is_null() {
        log::error!("characteristic_descriptor_disc_cb received null pointer");
        return -1;
    }

    let operation = unsafe { &*(operation as *const PeripheralOperation<BTreeSet<Descriptor>, M>) };
    if conn_handle != operation.conn_handle() {
        return 0;
    }

    let error = unsafe { &*error };

    if error.status == 0 {
        if dsc.is_null() {
            log::error!("characteristic_descriptor_disc_cb received null pointer for dsc");
            return -1;
        }

        let dsc = unsafe { &*dsc };
        match nimble_uuid_to_uuid(&dsc.uuid) {
            Ok(uuid) => {
                unsafe {
                    operation.context().lock_mut(|set| {
                        set.insert(Descriptor::new(uuid, dsc.handle));
                    })
                };
            }
            Err(e) => {
                operation.send_finished(Err(e.into()));
            }
        }
        return 0;
    }

    operation.send_finished(Ok(()));
    error.status as _
}
