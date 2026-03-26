use alloc::{boxed::Box, collections::BTreeSet, sync::Arc, vec::Vec};

use embassy_futures::select::{Either, select};
use embassy_sync::{
    blocking_mutex::{Mutex as BlockingMutex, raw::RawMutex},
    mutex::Mutex as AsyncMutex,
    pubsub::{PubSubChannel, Subscriber, WaitResult},
    signal::Signal,
};
use portable_atomic::{AtomicU16, Ordering};

use uuid::Uuid;

use crate::{
    characteristic::{Characteristic, Descriptor, read_attribute, write_attribute},
    data::BleAddr,
    discovery::{ServiceCharacteristicsDiscovery, ServiceDiscovery},
    error::{ConnectError, ConnectResult, GattError, GattResult, InternalError},
    nimble_sys::*,
    peripheral_operation::{PeripheralOperation, peripheral_operation},
    service::Service,
};

use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex as DefaultRawMutex;

/// We can only make one connection attempt at the same time.
static CONNECT_LOCK: AsyncMutex<DefaultRawMutex, ()> = AsyncMutex::new(());

/// Sentinel value: no active connection.
const CONN_HANDLE_NONE: u16 = u16::MAX;

/// Shared state for a peripheral, leaked to `'static`.
///
/// Lives as long as the program. The GAP callback holds a raw pointer to this,
/// so it must be at a stable address (Box::leak guarantees this).
struct PeripheralInner<M: RawMutex + 'static> {
    addr: BleAddr,

    /// Current connection handle, or `CONN_HANDLE_NONE` if disconnected.
    conn_handle: AtomicU16,

    /// One-shot signal for the current connect attempt.
    /// Written by `connect()`, consumed by the GAP callback.
    /// Safety: only mutated under CONNECT_LOCK (one attempt at a time),
    /// and the GAP callback only takes (not writes).
    connect_signal: BlockingMutex<M, Option<Arc<Signal<M, ConnectResult>>>>,

    /// Disconnect notifications (multi-subscriber).
    disconnect_pub: PubSubChannel<M, (), 4, 4, 1>,

    /// Discovered services cache.
    /// Safety: mutated from discovery (async, one at a time) and cleared
    /// from the disconnect callback (NimBLE host task). Both are serialised
    /// by the connection lifecycle — you can't discover while disconnected.
    services: BlockingMutex<M, BTreeSet<Service>>,

    /// Notifications/indications stream: (attr_handle, payload).
    subscription_pub: PubSubChannel<M, (u16, Vec<u8>), 16, 4, 1>,
}

impl<M: RawMutex + 'static> PeripheralInner<M> {
    /// Read the current connection handle, or `None` if disconnected.
    fn conn_handle(&self) -> Option<u16> {
        let h = self.conn_handle.load(Ordering::Acquire);
        if h == CONN_HANDLE_NONE { None } else { Some(h) }
    }
}

/// Handle to a BLE peripheral device.
///
/// Lightweight — holds a `&'static` reference to leaked inner state.
/// No heap allocation per clone.
pub struct Peripheral<M: RawMutex + 'static = DefaultRawMutex> {
    inner: &'static PeripheralInner<M>,
}

// Manual Clone: just copy the reference.
impl<M: RawMutex + 'static> Clone for Peripheral<M> {
    fn clone(&self) -> Self {
        Self { inner: self.inner }
    }
}

impl<M: RawMutex + 'static> Peripheral<M> {
    pub fn new(addr: BleAddr) -> Self {
        let inner: &'static PeripheralInner<M> = Box::leak(Box::new(PeripheralInner {
            addr,
            conn_handle: AtomicU16::new(CONN_HANDLE_NONE),
            connect_signal: BlockingMutex::new(None),
            disconnect_pub: PubSubChannel::new(),
            services: BlockingMutex::new(BTreeSet::new()),
            subscription_pub: PubSubChannel::new(),
        }));

        Self { inner }
    }

    pub async fn connect(&self) -> ConnectResult {
        log::info!("Connecting to peripheral at {:?}", self.inner.addr);

        let _guard = CONNECT_LOCK.lock().await;
        log::info!("Acquired connect lock for {:?}", self.inner.addr);

        let attempt = Arc::new(Signal::<M, ConnectResult>::new());
        // Safety: we hold CONNECT_LOCK, so no concurrent connect() can race.
        unsafe {
            self.inner.connect_signal.lock_mut(|slot| {
                *slot = Some(attempt.clone());
            })
        };

        let addr = self.inner.addr.clone().into();
        ble_gap_connect(
            0,
            &addr,
            1800,
            None,
            Some(Self::gap_event_handler),
            self.inner as *const PeripheralInner<M> as _,
        )
        .map_err(ConnectError::GapConnectFailed)?;

        let mut disc_sub = self
            .inner
            .disconnect_pub
            .subscriber()
            .map_err(|_| ConnectError::DisconnectedWhileOperation)?;

        log::info!("Waiting for connection result for {:?}", self.inner.addr);

        match select(attempt.wait(), disc_sub.next_message()).await {
            Either::First(result) => result,
            Either::Second(WaitResult::Message(())) | Either::Second(WaitResult::Lagged(_)) => {
                Err(ConnectError::DisconnectedWhileOperation)
            }
        }
    }

    pub async fn disconnect(&self) -> ConnectResult {
        log::debug!("Disconnecting");

        let Some(conn_handle) = self.inner.conn_handle() else {
            log::debug!("Not connected, nothing to disconnect");
            return Ok(());
        };

        let mut disc_sub = self
            .inner
            .disconnect_pub
            .subscriber()
            .map_err(|_| ConnectError::DisconnectedWhileOperation)?;

        ble_gap_terminate(
            conn_handle,
            bindings::ble_error_codes_BLE_ERR_REM_USER_CONN_TERM as _,
        )
        .map_err(ConnectError::DisconnectFailed)?;

        match disc_sub.next_message().await {
            WaitResult::Message(()) | WaitResult::Lagged(_) => Ok(()),
        }
    }

    pub fn is_connected(&self) -> bool {
        self.inner.conn_handle().is_some()
    }

    pub async fn exchange_mtu(&self) -> GattResult {
        let Some(conn_handle) = self.inner.conn_handle() else {
            return Err(GattError::NotConnected);
        };

        let (operation, operation_handle) = peripheral_operation::<u16, M>(conn_handle, 0);

        ble_gattc_exchange_mtu(
            conn_handle,
            Some(Self::exchange_mtu_callback),
            &operation as *const PeripheralOperation<u16, M> as _,
        )
        .map_err(GattError::MtuExchangeFailed)?;

        operation_handle.join().await
    }

    /// Discover a specific service by UUID and cache it in `self.services`.
    pub async fn discover_service_by_uuid(&self, uuid: &Uuid) -> GattResult<()> {
        let Some(conn_handle) = self.inner.conn_handle() else {
            return Err(GattError::NotConnected);
        };

        let discovery = ServiceCharacteristicsDiscovery::new(conn_handle, uuid);

        let mut disc_sub = self
            .inner
            .disconnect_pub
            .subscriber()
            .map_err(|_| GattError::DisconnectedWhileOperation)?;

        let result = match select(discovery.run(), disc_sub.next_message()).await {
            Either::First(r) => r,
            Either::Second(WaitResult::Message(())) | Either::Second(WaitResult::Lagged(_)) => {
                return Err(GattError::DisconnectedWhileOperation);
            }
        };

        if let Some(service) = result? {
            // Safety: discovery is serialised by connection lifecycle.
            unsafe {
                self.inner.services.lock_mut(|s| {
                    s.insert(service);
                })
            };
        }

        Ok(())
    }

    /// Discover all services and replace the cache.
    pub async fn discover_all_services(&self) -> GattResult<()> {
        let Some(conn_handle) = self.inner.conn_handle() else {
            return Err(GattError::NotConnected);
        };

        let discovery = ServiceDiscovery::new(conn_handle);

        let mut disc_sub = self
            .inner
            .disconnect_pub
            .subscriber()
            .map_err(|_| GattError::DisconnectedWhileOperation)?;

        let result = match select(discovery.run(), disc_sub.next_message()).await {
            Either::First(r) => r,
            Either::Second(WaitResult::Message(())) | Either::Second(WaitResult::Lagged(_)) => {
                return Err(GattError::DisconnectedWhileOperation);
            }
        };

        let services = result?;
        // Safety: discovery is serialised by connection lifecycle.
        unsafe { self.inner.services.lock_mut(|s| *s = services) };

        Ok(())
    }

    /// Returns a snapshot of the cached services.
    pub fn services(&self) -> BTreeSet<Service> {
        self.inner.services.lock(|s| s.iter().cloned().collect())
    }

    pub async fn read(&self, characteristic: &Characteristic) -> GattResult<bytes::Bytes> {
        let Some(conn_handle) = self.inner.conn_handle() else {
            return Err(GattError::NotConnected);
        };

        read_attribute(conn_handle, characteristic.handle()).await
    }

    pub async fn read_descriptor(&self, descriptor: &Descriptor) -> GattResult<bytes::Bytes> {
        let Some(conn_handle) = self.inner.conn_handle() else {
            return Err(GattError::NotConnected);
        };

        read_attribute(conn_handle, descriptor.handle()).await
    }

    pub async fn write(
        &self,
        characteristic: &Characteristic,
        data: &[u8],
        response: bool,
    ) -> GattResult {
        let Some(conn_handle) = self.inner.conn_handle() else {
            return Err(GattError::NotConnected);
        };

        write_attribute(
            conn_handle,
            characteristic.handle(),
            Arc::from(Box::<[u8]>::from(data)),
            response,
        )
        .await
    }

    pub async fn write_descriptor(
        &self,
        descriptor: &Descriptor,
        data: &[u8],
        response: bool,
    ) -> GattResult {
        let Some(conn_handle) = self.inner.conn_handle() else {
            return Err(GattError::NotConnected);
        };

        write_attribute(
            conn_handle,
            descriptor.handle(),
            Arc::from(Box::<[u8]>::from(data)),
            response,
        )
        .await
    }

    /// Subscribe to notifications/indications.
    pub fn subscribe(
        &self,
    ) -> core::result::Result<Subscriber<'_, M, (u16, Vec<u8>), 16, 4, 1>, InternalError> {
        self.inner
            .subscription_pub
            .subscriber()
            .map_err(|_| InternalError::ChannelClosed)
    }

    unsafe extern "C" fn gap_event_handler(
        event: *mut bindings::ble_gap_event,
        param: *mut core::ffi::c_void,
    ) -> i32 {
        let inner = unsafe { &*(param as *const PeripheralInner<M>) };
        let event = unsafe { *event };

        match event.type_ as u32 {
            bindings::BLE_GAP_EVENT_CONNECT => handle_connect(inner, &event),
            bindings::BLE_GAP_EVENT_DISCONNECT => handle_disconnect(inner, &event),
            bindings::BLE_GAP_EVENT_NOTIFY_RX => handle_notify_rx(inner, &event),
            _ => 0,
        }
    }

    unsafe extern "C" fn exchange_mtu_callback(
        conn_handle: u16,
        error: *const bindings::ble_gatt_error,
        mtu: u16,
        operation: *mut core::ffi::c_void,
    ) -> i32 {
        let operation = unsafe { &*(operation as *const PeripheralOperation<u16, M>) };
        let error = unsafe { &*error };

        if conn_handle != operation.conn_handle() {
            return 0;
        }

        let result = return_code_to_result(error.status as u32, ()).map_err(GattError::MtuExchangeFailed);

        if result.is_ok() {
            // Safety: callback is single-threaded (NimBLE host task).
            unsafe { operation.context().lock_mut(|v| *v = mtu) };
        }

        operation.send_finished(result);
        error.status as _
    }

    pub fn addr(&self) -> &BleAddr {
        &self.inner.addr
    }
}

fn handle_connect<M: RawMutex>(inner: &PeripheralInner<M>, event: &bindings::ble_gap_event) -> i32 {
    let connect = unsafe { &event.__bindgen_anon_1.connect };
    let result = return_code_to_result(connect.status as u32, ()).map_err(ConnectError::GapConnectFailed);

    if result.is_ok() {
        let current = inner.conn_handle.load(Ordering::Acquire);

        if current != CONN_HANDLE_NONE {
            log::info!("Already connected with handle {current}, ignoring connect event");
        } else {
            log::info!("Connected with handle {}", connect.conn_handle);
            inner.conn_handle.store(connect.conn_handle, Ordering::Release);
        }
    } else {
        log::error!("Failed to connect: {:?}", result.as_ref().unwrap_err());
    }

    // Safety: GAP callback is single-threaded (NimBLE host task).
    let attempt = unsafe { inner.connect_signal.lock_mut(|slot| slot.take()) };
    if let Some(sig) = attempt {
        sig.signal(result);
    } else {
        log::error!("No connect attempt signal present");
    }

    0
}

fn handle_disconnect<M: RawMutex>(
    inner: &PeripheralInner<M>,
    event: &bindings::ble_gap_event,
) -> i32 {
    let disconnect = unsafe { &event.__bindgen_anon_1.disconnect };

    let current = inner.conn_handle.load(Ordering::Acquire);

    if current != CONN_HANDLE_NONE {
        log::debug!("Disconnected from handle {}", current);

        if current != disconnect.conn.conn_handle {
            log::warn!(
                "Received disconnect for handle {}, current handle is {}",
                disconnect.conn.conn_handle,
                current
            );
            return 0;
        }

        inner.conn_handle.store(CONN_HANDLE_NONE, Ordering::Release);

        // Clear cached services on disconnect.
        // Safety: NimBLE host task is single-threaded.
        unsafe { inner.services.lock_mut(|s| s.clear()) };

        if let Ok(p) = inner.disconnect_pub.publisher() {
            p.publish_immediate(());
        }
    } else {
        log::debug!("Not connected, ignoring disconnect event");
    }

    0
}

fn handle_notify_rx<M: RawMutex>(
    inner: &PeripheralInner<M>,
    event: &bindings::ble_gap_event,
) -> i32 {
    let notify_rx = unsafe { &event.__bindgen_anon_1.notify_rx };

    let om = notify_rx.om;
    let attr_handle = notify_rx.attr_handle;
    let _conn_handle = notify_rx.conn_handle;

    let payload = match ble_hs_mbuf_to_flat(om) {
        Ok(payload) => payload,
        Err(e) => {
            log::error!("Failed to convert buffer to flat: {e}");
            return 0;
        }
    };

    if let Ok(p) = inner.subscription_pub.publisher() {
        p.publish_immediate((attr_handle, payload));
    }

    0
}
