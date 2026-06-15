use alloc::{boxed::Box, sync::Arc, vec::Vec};
use core::cell::RefCell;

use embassy_futures::select::{Either, select};
use embassy_sync::{
    blocking_mutex::{Mutex as BlockingMutex, raw::RawMutex},
    mutex::Mutex as AsyncMutex,
    pubsub::{PubSubChannel, Subscriber, WaitResult},
    signal::Signal,
};
use portable_atomic::{AtomicU16, AtomicU32, Ordering};

use uuid::Uuid;

use crate::{
    characteristic::{Characteristic, Descriptor, read_attribute, write_attribute},
    data::BleAddr,
    discovery::{ServiceCharacteristicsDiscovery, ServiceDiscovery},
    error::{ConnectError, ConnectResult, GattError, GattResult, InternalError, PairError, PairResult},
    nimble_sys::*,
    peripheral_operation::{PeripheralOperation, peripheral_operation},
    service::Service,
};

use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex as DefaultRawMutex;

/// Serialises connection *establishment* across all `Peripheral`s.
///
/// The controller cannot have two connection attempts in flight at once — a second
/// `ble_gap_connect()` while one is pending fails immediately. This lock makes
/// concurrent `connect()` calls (e.g. to different peripherals) queue instead of
/// erroring. It does NOT limit how many connections can be *held* simultaneously;
/// that is governed by `max_connections` in nimble-config.toml.
static CONNECT_LOCK: AsyncMutex<DefaultRawMutex, ()> = AsyncMutex::new(());

/// Sentinel value: no active connection.
const CONN_HANDLE_NONE: u16 = u16::MAX;

/// Connection-establishment timeout passed to `ble_gap_connect`, in milliseconds.
///
/// On expiry NimBLE ends the procedure itself and reports a CONNECT event with
/// `BLE_HS_ETIMEOUT`, which we surface as [`ConnectError::Timeout`]. We rely on this
/// stack-level timeout rather than a separate Rust-side timer. `0` would mean
/// "no timeout" (attempt forever).
const CONNECT_TIMEOUT_MS: u32 = 1800;

/// Connection-level events surfaced to the application via [`Peripheral::events`].
#[derive(Clone, Copy, Debug)]
pub enum PeripheralEvent {
    /// The ATT MTU for the connection changed (negotiated or peer-initiated).
    MtuChanged { mtu: u16 },
    /// Connection parameters were updated. `status` is 0 on success.
    ConnParamsUpdated { status: u32 },
    /// The connection was terminated. `reason` is the HCI/host reason code.
    Disconnected { reason: u32 },
}

/// Shared state for a peripheral, reference-counted via [`Arc`].
///
/// While a connection is being established or held, the NimBLE GAP callback owns
/// one additional strong reference (handed over as a raw pointer in [`Peripheral::connect`]
/// and reclaimed on the terminal CONNECT-failure or DISCONNECT event), keeping the
/// allocation at a stable address for as long as the callback can fire. The
/// allocation is freed once the callback reference and every `Peripheral` /
/// subscriber handle have been dropped.
struct PeripheralInner<M: RawMutex + 'static> {
    addr: BleAddr,

    /// Current connection handle, or `CONN_HANDLE_NONE` if disconnected.
    conn_handle: AtomicU16,

    /// Result of the current connect attempt.
    /// `connect()` calls `reset()` then `wait()`; the GAP callback signals it.
    /// CONNECT_LOCK serialises attempts, so a single permanent signal suffices
    /// (no per-attempt allocation).
    connect_result: Signal<M, ConnectResult>,

    /// Disconnect notifications (multi-subscriber).
    disconnect_pub: PubSubChannel<M, (), 4, 4, 1>,

    /// Discovered services cache.
    /// Mutated from discovery (async, one at a time) and cleared from the
    /// disconnect callback (NimBLE host task); the `Mutex` serialises the two
    /// and `RefCell` provides safe interior mutability.
    services: BlockingMutex<M, RefCell<Vec<Service>>>,

    /// Notifications/indications stream: (attr_handle, payload).
    subscription_pub: PubSubChannel<M, (u16, Vec<u8>), 16, 4, 1>,

    /// Connection-level events (MTU change, conn-param update, disconnect).
    event_pub: PubSubChannel<M, PeripheralEvent, 4, 2, 1>,

    /// Passkey stored before `pair_with_passkey` initiates; injected on PASSKEY_ACTION.
    static_passkey: AtomicU32,

    /// Signals the result of an in-progress `pair_with_passkey` call.
    pair_result: Signal<M, PairResult>,
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
/// Cheap to clone — it is an [`Arc`] over the shared state, so a clone only bumps
/// a refcount. The underlying allocation is reclaimed once the last handle (and
/// the connection callback's reference) is dropped.
pub struct Peripheral<M: RawMutex + 'static = DefaultRawMutex> {
    inner: Arc<PeripheralInner<M>>,
}

impl<M: RawMutex + 'static> Clone for Peripheral<M> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

/// A subscriber that owns a strong reference to the peripheral's shared state.
///
/// Returned by [`Peripheral::subscribe`] / [`Peripheral::events`]. It keeps the
/// underlying allocation alive for as long as it is held, so it stays valid even
/// if every [`Peripheral`] handle is dropped while the subscriber is still in use.
pub struct OwnedSubscriber<
    M: RawMutex + 'static,
    T: Clone + 'static,
    const CAP: usize,
    const SUBS: usize,
    const PUBS: usize,
> {
    // Declaration order is load-bearing: `sub` borrows into a channel owned by
    // `_inner` and must be dropped first. Rust drops fields in declaration order.
    sub: Subscriber<'static, M, T, CAP, SUBS, PUBS>,
    _inner: Arc<PeripheralInner<M>>,
}

impl<
    M: RawMutex + 'static,
    T: Clone + 'static,
    const CAP: usize,
    const SUBS: usize,
    const PUBS: usize,
> OwnedSubscriber<M, T, CAP, SUBS, PUBS>
{
    /// Wait for the next message on this subscription.
    pub async fn next_message(&mut self) -> WaitResult<T> {
        self.sub.next_message().await
    }
}

/// Notification/indication subscriber returned by [`Peripheral::subscribe`].
pub type NotificationSubscriber<M = DefaultRawMutex> = OwnedSubscriber<M, (u16, Vec<u8>), 16, 4, 1>;
/// Connection-event subscriber returned by [`Peripheral::events`].
pub type EventSubscriber<M = DefaultRawMutex> = OwnedSubscriber<M, PeripheralEvent, 4, 2, 1>;

impl<M: RawMutex + 'static> Peripheral<M> {
    pub fn new(addr: BleAddr) -> Self {
        Self {
            inner: Arc::new(PeripheralInner {
                addr,
                conn_handle: AtomicU16::new(CONN_HANDLE_NONE),
                connect_result: Signal::new(),
                disconnect_pub: PubSubChannel::new(),
                services: BlockingMutex::new(RefCell::new(Vec::new())),
                subscription_pub: PubSubChannel::new(),
                event_pub: PubSubChannel::new(),
                static_passkey: AtomicU32::new(0),
                pair_result: Signal::new(),
            }),
        }
    }

    pub async fn connect(&self) -> ConnectResult {
        log::info!("Connecting to peripheral at {:?}", self.inner.addr);

        let _guard = CONNECT_LOCK.lock().await;
        log::info!("Acquired connect lock for {:?}", self.inner.addr);

        // We hold CONNECT_LOCK, so no concurrent connect() can race. Clear any
        // stale result before arming the GAP connect.
        self.inner.connect_result.reset();

        // Hand a strong reference to the GAP callback as a raw pointer. It is
        // reclaimed on the terminal event (failed CONNECT or DISCONNECT) inside
        // `gap_event_handler`, keeping the allocation alive for as long as the
        // callback can fire.
        let raw = Arc::into_raw(self.inner.clone());

        let addr = self.inner.addr.clone().into();
        if let Err(e) = ble_gap_connect(
            0,
            &addr,
            CONNECT_TIMEOUT_MS,
            None,
            Some(Self::gap_event_handler),
            raw as *mut core::ffi::c_void,
        ) {
            // The callback was never registered, so no terminal event will reclaim
            // the reference — drop it here to avoid leaking the allocation.
            // SAFETY: `raw` came from `Arc::into_raw` directly above and has not been
            // handed to NimBLE, so this is the sole owner of that strong reference.
            drop(unsafe { Arc::from_raw(raw) });
            return Err(ConnectError::GapConnectFailed(e));
        }

        let mut disc_sub = self
            .inner
            .disconnect_pub
            .subscriber()
            .map_err(|_| ConnectError::DisconnectedWhileOperation)?;

        log::info!("Waiting for connection result for {:?}", self.inner.addr);

        match select(self.inner.connect_result.wait(), disc_sub.next_message()).await {
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

    /// Initiate Legacy passkey pairing on an established connection.
    ///
    /// Stores `passkey`, resets the pair signal, calls
    /// `ble_gap_security_initiate`, then awaits `BLE_GAP_EVENT_ENC_CHANGE`
    /// (or a disconnect). The passkey is injected automatically when
    /// `BLE_GAP_EVENT_PASSKEY_ACTION` fires in the GAP callback.
    pub async fn pair_with_passkey(&self, passkey: u32) -> PairResult {
        let Some(conn_handle) = self.inner.conn_handle() else {
            return Err(PairError::NotConnected);
        };

        self.inner.static_passkey.store(passkey, Ordering::Release);
        self.inner.pair_result.reset();

        ble_gap_security_initiate(conn_handle).map_err(PairError::InitiateFailed)?;

        let mut disc_sub = self
            .inner
            .disconnect_pub
            .subscriber()
            .map_err(|_| PairError::DisconnectedWhileOperation)?;

        match select(self.inner.pair_result.wait(), disc_sub.next_message()).await {
            Either::First(result) => result,
            Either::Second(WaitResult::Message(())) | Either::Second(WaitResult::Lagged(_)) => {
                Err(PairError::DisconnectedWhileOperation)
            }
        }
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
            self.inner.services.lock(|s| s.borrow_mut().push(service));
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
        self.inner.services.lock(|s| *s.borrow_mut() = services);

        Ok(())
    }

    /// Returns a snapshot of the cached services.
    pub fn services(&self) -> Vec<Service> {
        self.inner.services.lock(|s| s.borrow().clone())
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
    ///
    /// The returned subscriber owns a strong reference to the peripheral, so it
    /// stays valid even if every [`Peripheral`] handle is dropped while it is held.
    pub fn subscribe(&self) -> core::result::Result<NotificationSubscriber<M>, InternalError> {
        let sub = self
            .inner
            .subscription_pub
            .subscriber()
            .map_err(|_| InternalError::ChannelClosed)?;
        // SAFETY: the `_inner` clone keeps the PubSubChannel alive for as long as the
        // returned subscriber exists, and `OwnedSubscriber` drops `sub` before `_inner`
        // (field declaration order), so the extended `'static` lifetime never dangles.
        let sub: Subscriber<'static, M, (u16, Vec<u8>), 16, 4, 1> =
            unsafe { core::mem::transmute(sub) };
        Ok(OwnedSubscriber {
            sub,
            _inner: self.inner.clone(),
        })
    }

    /// Subscribe to connection-level events (MTU change, conn-param update, disconnect).
    ///
    /// The returned subscriber owns a strong reference to the peripheral, so it
    /// stays valid even if every [`Peripheral`] handle is dropped while it is held.
    pub fn events(&self) -> core::result::Result<EventSubscriber<M>, InternalError> {
        let sub = self
            .inner
            .event_pub
            .subscriber()
            .map_err(|_| InternalError::ChannelClosed)?;
        // SAFETY: see `subscribe` — `_inner` keeps the channel alive and `sub` is
        // dropped before `_inner`.
        let sub: Subscriber<'static, M, PeripheralEvent, 4, 2, 1> =
            unsafe { core::mem::transmute(sub) };
        Ok(OwnedSubscriber {
            sub,
            _inner: self.inner.clone(),
        })
    }

    unsafe extern "C" fn gap_event_handler(
        event: *mut bindings::ble_gap_event,
        param: *mut core::ffi::c_void,
    ) -> i32 {
        let inner = unsafe { &*(param as *const PeripheralInner<M>) };
        let event = unsafe { *event };

        match event.type_ as u32 {
            bindings::BLE_GAP_EVENT_PASSKEY_ACTION => handle_passkey_action(inner, &event),
            bindings::BLE_GAP_EVENT_ENC_CHANGE => handle_enc_change(inner, &event),
            bindings::BLE_GAP_EVENT_REPEAT_PAIRING => {
                // bonding=0 means no stored bonds — should never fire; ignore safely.
                log::warn!("[peripheral] REPEAT_PAIRING received with bonding disabled — ignoring");
                bindings::BLE_GAP_REPEAT_PAIRING_IGNORE as i32
            }
            bindings::BLE_GAP_EVENT_CONNECT => {
                let established = handle_connect(inner, &event);
                if !established {
                    // A failed connect is terminal — no DISCONNECT follows — so this
                    // is the last event for the reference handed over in `connect()`.
                    // SAFETY: balances the `Arc::into_raw` in `connect()`. NimBLE
                    // delivers exactly one CONNECT result per attempt, so this runs
                    // once. `inner` is not touched after the drop.
                    drop(unsafe { Arc::from_raw(param as *const PeripheralInner<M>) });
                }
                0
            }
            bindings::BLE_GAP_EVENT_DISCONNECT => {
                handle_disconnect(inner, &event);
                // Disconnect is the terminal event for an established connection.
                // SAFETY: balances the `Arc::into_raw` in `connect()`. NimBLE delivers
                // exactly one DISCONNECT for the connection owned by this callback, so
                // this runs once. `inner` is not touched after the drop.
                drop(unsafe { Arc::from_raw(param as *const PeripheralInner<M>) });
                0
            }
            bindings::BLE_GAP_EVENT_NOTIFY_RX => handle_notify_rx(inner, &event),
            bindings::BLE_GAP_EVENT_MTU => handle_mtu(inner, &event),
            bindings::BLE_GAP_EVENT_CONN_UPDATE => handle_conn_update(inner, &event),
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

        let result =
            return_code_to_result(error.status as u32, ()).map_err(GattError::MtuExchangeFailed);

        if result.is_ok() {
            operation.context().lock(|v| *v.borrow_mut() = mtu);
        }

        operation.send_finished(result);
        error.status as _
    }

    pub fn addr(&self) -> &BleAddr {
        &self.inner.addr
    }
}

/// Handles a CONNECT event. Returns `true` if a connection was established (the
/// caller must then keep the callback's reference alive until DISCONNECT), or
/// `false` on failure (the attempt is terminal and the reference is released).
fn handle_connect<M: RawMutex>(
    inner: &PeripheralInner<M>,
    event: &bindings::ble_gap_event,
) -> bool {
    let connect = unsafe { &event.__bindgen_anon_1.connect };
    // A connect-duration expiry comes back as BLE_HS_ETIMEOUT; surface it as the
    // semantic `Timeout` so callers can match it without inspecting the raw code.
    let result = match return_code_to_result(connect.status as u32, ()) {
        Ok(()) => Ok(()),
        Err(NimbleError::Timeout) => Err(ConnectError::Timeout),
        Err(e) => Err(ConnectError::GapConnectFailed(e)),
    };

    let established = result.is_ok();

    if established {
        let current = inner.conn_handle.load(Ordering::Acquire);

        if current != CONN_HANDLE_NONE {
            log::info!("Already connected with handle {current}, ignoring connect event");
        } else {
            log::info!("Connected with handle {}", connect.conn_handle);
            inner
                .conn_handle
                .store(connect.conn_handle, Ordering::Release);
        }
    } else {
        log::error!("Failed to connect: {:?}", result.as_ref().unwrap_err());
    }

    inner.connect_result.signal(result);

    established
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
        inner.services.lock(|s| s.borrow_mut().clear());

        // Internal abort signal for in-flight connect/discover operations.
        if let Ok(p) = inner.disconnect_pub.publisher() {
            p.publish_immediate(());
        }

        // External connection-event stream.
        if let Ok(p) = inner.event_pub.publisher() {
            p.publish_immediate(PeripheralEvent::Disconnected {
                reason: disconnect.reason as u32,
            });
        }
    } else {
        log::debug!("Not connected, ignoring disconnect event");
    }

    0
}

fn handle_mtu<M: RawMutex>(inner: &PeripheralInner<M>, event: &bindings::ble_gap_event) -> i32 {
    let mtu = unsafe { &event.__bindgen_anon_1.mtu };

    log::debug!("MTU changed to {} on handle {}", mtu.value, mtu.conn_handle);

    if let Ok(p) = inner.event_pub.publisher() {
        p.publish_immediate(PeripheralEvent::MtuChanged { mtu: mtu.value });
    }

    0
}

fn handle_conn_update<M: RawMutex>(
    inner: &PeripheralInner<M>,
    event: &bindings::ble_gap_event,
) -> i32 {
    let conn_update = unsafe { &event.__bindgen_anon_1.conn_update };

    log::debug!(
        "Connection params updated (status {}) on handle {}",
        conn_update.status,
        conn_update.conn_handle
    );

    if let Ok(p) = inner.event_pub.publisher() {
        p.publish_immediate(PeripheralEvent::ConnParamsUpdated {
            status: conn_update.status as u32,
        });
    }

    0
}

fn handle_passkey_action<M: RawMutex>(
    inner: &PeripheralInner<M>,
    event: &bindings::ble_gap_event,
) -> i32 {
    let pk = unsafe { &event.__bindgen_anon_1.passkey };

    let action = pk.params.action as u32;
    if action != bindings::BLE_SM_IOACT_INPUT && action != bindings::BLE_SM_IOACT_DISP {
        log::warn!(
            "[peripheral] Unexpected passkey IO action {} — only INPUT/DISP supported",
            pk.params.action
        );
        // SM will time out; ENC_CHANGE fires with a non-zero status.
        return 0;
    }

    let passkey_val = inner.static_passkey.load(Ordering::Acquire);
    let mut io: bindings::ble_sm_io = unsafe { core::mem::zeroed() };
    io.action = action as u8;  // INPUT or DISP — same injection mechanism
    io.__bindgen_anon_1.passkey = passkey_val;

    if let Err(e) = ble_sm_inject_io(pk.conn_handle, &mut io) {
        log::error!("[peripheral] ble_sm_inject_io failed: {e:?}");
    }

    0
}

fn handle_enc_change<M: RawMutex>(
    inner: &PeripheralInner<M>,
    event: &bindings::ble_gap_event,
) -> i32 {
    let enc = unsafe { &event.__bindgen_anon_1.enc_change };

    let result = if enc.status == 0 {
        log::info!("[peripheral] ENC_CHANGE: link encrypted on handle {}", enc.conn_handle);
        Ok(())
    } else {
        log::warn!("[peripheral] ENC_CHANGE: pairing failed, status={}", enc.status);
        Err(PairError::PairingFailed { status: enc.status as u32 })
    };

    // Signal any waiting pair_with_passkey call. If no pairing was in progress
    // (e.g. peripheral-initiated encryption), pair_result.reset() on the next
    // pair_with_passkey call will clear this harmlessly.
    inner.pair_result.signal(result);

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
