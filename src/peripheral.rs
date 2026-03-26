use alloc::{boxed::Box, collections::BTreeSet, sync::Arc, vec::Vec};

use embassy_futures::select::{Either, select};
use embassy_sync::{
    blocking_mutex::{Mutex as BlockingMutex, raw::RawMutex},
    mutex::Mutex as AsyncMutex,
    pubsub::{PubSubChannel, Subscriber, WaitResult},
    signal::Signal,
};

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

#[derive(Debug, Clone)]
enum ConnectionState {
    Disconnected,
    Connected(u16),
}

impl ConnectionState {
    #[must_use]
    fn is_connected(&self) -> bool {
        matches!(self, Self::Connected(..))
    }
}

#[derive(Clone)]
pub struct Peripheral<M: RawMutex = DefaultRawMutex> {
    addr: BleAddr,

    connection_state: Arc<BlockingMutex<M, ConnectionState>>,

    /// Slot that holds the current connect-attempt signal, if any.
    connect_signal: Arc<BlockingMutex<M, Option<Arc<Signal<M, ConnectResult>>>>>,

    /// Disconnect notifications (multi-subscriber).
    disconnect_pub: Arc<PubSubChannel<M, (), 4, 4, 1>>,

    /// Discovered services cache.
    services: Arc<BlockingMutex<M, BTreeSet<Service>>>,

    /// Notifications/indications stream: (attr_handle, payload).
    subscription_pub: Arc<PubSubChannel<M, (u16, Vec<u8>), 16, 4, 1>>,
}

impl<M: RawMutex> Peripheral<M> {
    pub fn new(addr: BleAddr) -> Self {
        Self {
            addr,
            connection_state: Arc::new(BlockingMutex::new(ConnectionState::Disconnected)),
            connect_signal: Arc::new(BlockingMutex::new(None)),
            disconnect_pub: Arc::new(PubSubChannel::new()),
            services: Arc::new(BlockingMutex::new(BTreeSet::new())),
            subscription_pub: Arc::new(PubSubChannel::new()),
        }
    }

    pub async fn connect(&self) -> ConnectResult {
        log::info!("Connecting to peripheral at {:?}", self.addr);

        let _guard = CONNECT_LOCK.lock().await;
        log::info!("Acquired connect lock for {:?}", self.addr);

        let attempt = Arc::new(Signal::<M, ConnectResult>::new());
        unsafe {
            self.connect_signal.lock_mut(|slot| {
                *slot = Some(attempt.clone());
            })
        };

        let addr = self.addr.clone().into();
        ble_gap_connect(
            0,
            &addr,
            1800,
            None,
            Some(Self::gap_event_handler),
            self as *const Self as _,
        )
        .map_err(ConnectError::GapConnectFailed)?;

        let mut disc_sub = self
            .disconnect_pub
            .subscriber()
            .map_err(|_| ConnectError::DisconnectedWhileOperation)?;

        log::info!("Waiting for connection result for {:?}", self.addr);

        match select(attempt.wait(), disc_sub.next_message()).await {
            Either::First(result) => result,
            Either::Second(WaitResult::Message(())) | Either::Second(WaitResult::Lagged(_)) => {
                Err(ConnectError::DisconnectedWhileOperation)
            }
        }
    }

    pub async fn disconnect(&self) -> ConnectResult {
        log::debug!("Disconnecting");

        let conn_handle = self.connection_state.lock(|s| match *s {
            ConnectionState::Connected(h) => Some(h),
            ConnectionState::Disconnected => None,
        });

        let Some(conn_handle) = conn_handle else {
            log::debug!("Not connected, nothing to disconnect");
            return Ok(());
        };

        let mut disc_sub = self
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
        self.connection_state.lock(|s| s.is_connected())
    }

    pub async fn exchange_mtu(&self) -> GattResult {
        let conn_handle = self.connection_state.lock(|s| match *s {
            ConnectionState::Connected(h) => Some(h),
            ConnectionState::Disconnected => None,
        });

        let Some(conn_handle) = conn_handle else {
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
        let conn_handle = self.connection_state.lock(|s| match *s {
            ConnectionState::Connected(h) => Some(h),
            ConnectionState::Disconnected => None,
        });
        let Some(conn_handle) = conn_handle else {
            return Err(GattError::NotConnected);
        };

        let discovery = ServiceCharacteristicsDiscovery::new(conn_handle, uuid);

        let mut disc_sub = self
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
            unsafe {
                self.services.lock_mut(|s| {
                    s.insert(service);
                })
            };
        }

        Ok(())
    }

    /// Discover all services and replace the cache.
    pub async fn discover_all_services(&self) -> GattResult<()> {
        let conn_handle = self.connection_state.lock(|s| match *s {
            ConnectionState::Connected(h) => Some(h),
            ConnectionState::Disconnected => None,
        });
        let Some(conn_handle) = conn_handle else {
            return Err(GattError::NotConnected);
        };

        let discovery = ServiceDiscovery::new(conn_handle);

        let mut disc_sub = self
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
        unsafe { self.services.lock_mut(|s| *s = services) };

        Ok(())
    }

    /// Returns a snapshot of the cached services.
    pub async fn services(&self) -> BTreeSet<Service> {
        self.services.lock(|s| s.iter().cloned().collect())
    }

    pub async fn read(&self, characteristic: &Characteristic) -> GattResult<bytes::Bytes> {
        let conn_handle = self.connection_state.lock(|s| match *s {
            ConnectionState::Connected(h) => Some(h),
            ConnectionState::Disconnected => None,
        });
        let Some(conn_handle) = conn_handle else {
            return Err(GattError::NotConnected);
        };

        read_attribute(conn_handle, characteristic.handle()).await
    }

    pub async fn read_descriptor(&self, descriptor: &Descriptor) -> GattResult<bytes::Bytes> {
        let conn_handle = self.connection_state.lock(|s| match *s {
            ConnectionState::Connected(h) => Some(h),
            ConnectionState::Disconnected => None,
        });
        let Some(conn_handle) = conn_handle else {
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
        let conn_handle = self.connection_state.lock(|s| match *s {
            ConnectionState::Connected(h) => Some(h),
            ConnectionState::Disconnected => None,
        });
        let Some(conn_handle) = conn_handle else {
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
        let conn_handle = self.connection_state.lock(|s| match *s {
            ConnectionState::Connected(h) => Some(h),
            ConnectionState::Disconnected => None,
        });
        let Some(conn_handle) = conn_handle else {
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
    ///
    /// The returned subscriber borrows `self` (lifetime tied to `&self`).
    pub fn subscribe(
        &self,
    ) -> core::result::Result<Subscriber<'_, M, (u16, Vec<u8>), 16, 4, 1>, InternalError> {
        self.subscription_pub
            .subscriber()
            .map_err(|_| InternalError::ChannelClosed)
    }

    unsafe extern "C" fn gap_event_handler(
        event: *mut bindings::ble_gap_event,
        param: *mut core::ffi::c_void,
    ) -> i32 {
        let peripheral = unsafe { &*(param as *const Self) };
        let event = unsafe { *event };

        match event.type_ as u32 {
            bindings::BLE_GAP_EVENT_CONNECT => handle_connect(peripheral, &event),
            bindings::BLE_GAP_EVENT_DISCONNECT => handle_disconnect(peripheral, &event),
            bindings::BLE_GAP_EVENT_NOTIFY_RX => handle_notify_rx(peripheral, &event),
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
            unsafe { operation.context().lock_mut(|v| *v = mtu) };
        }

        operation.send_finished(result);
        error.status as _
    }

    pub fn addr(&self) -> &BleAddr {
        &self.addr
    }
}

impl<M: RawMutex> Drop for Peripheral<M> {
    fn drop(&mut self) {
        // Same as before: keep Drop lock-free / best-effort.
        // If you want termination in Drop, add a cached conn_handle (AtomicU16 + flag)
        // updated in connect/disconnect handlers.
    }
}

fn handle_connect<M: RawMutex>(peripheral: &Peripheral<M>, event: &bindings::ble_gap_event) -> i32 {
    let connect = unsafe { &event.__bindgen_anon_1.connect };
    let result = return_code_to_result(connect.status as u32, ()).map_err(ConnectError::GapConnectFailed);

    if result.is_ok() {
        let already = peripheral
            .connection_state
            .lock(|s| matches!(*s, ConnectionState::Connected(_)));

        if already {
            peripheral.connection_state.lock(|s| {
                if let ConnectionState::Connected(existing) = *s {
                    log::info!("Already connected with handle {existing}, ignoring connect event");
                }
            });
        } else {
            log::info!("Connected with handle {}", connect.conn_handle);
            unsafe {
                peripheral
                    .connection_state
                    .lock_mut(|s| *s = ConnectionState::Connected(connect.conn_handle))
            };
        }
    } else {
        log::error!("Failed to connect: {:?}", result.as_ref().unwrap_err());
    }

    let attempt = unsafe { peripheral.connect_signal.lock_mut(|slot| slot.take()) };
    if let Some(sig) = attempt {
        sig.signal(result);
    } else {
        log::error!("No connect attempt signal present");
    }

    0
}

fn handle_disconnect<M: RawMutex>(
    peripheral: &Peripheral<M>,
    event: &bindings::ble_gap_event,
) -> i32 {
    let disconnect = unsafe { &event.__bindgen_anon_1.disconnect };

    let state = peripheral.connection_state.lock(|s| s.clone());

    if let ConnectionState::Connected(conn_handle) = state {
        log::debug!("Disconnected from handle {}", conn_handle);

        if conn_handle != disconnect.conn.conn_handle {
            log::warn!(
                "Received disconnect for handle {}, current handle is {}",
                disconnect.conn.conn_handle,
                conn_handle
            );
            return 0;
        }

        unsafe {
            peripheral
                .connection_state
                .lock_mut(|s| *s = ConnectionState::Disconnected)
        };

        // Clear cached services on disconnect (matches typical expectations).
        unsafe { peripheral.services.lock_mut(|s| s.clear()) };

        if let Ok(p) = peripheral.disconnect_pub.publisher() {
            p.publish_immediate(());
        }
    } else {
        log::debug!("Not connected, ignoring disconnect event");
    }

    0
}

fn handle_notify_rx<M: RawMutex>(
    peripheral: &Peripheral<M>,
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

    if let Ok(p) = peripheral.subscription_pub.publisher() {
        // If the channel is full, PubSubChannel will drop/lag depending on config;
        // keep it best-effort like broadcast.
        p.publish_immediate((attr_handle, payload));
    }

    0
}
