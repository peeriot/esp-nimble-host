#![no_std]
#![feature(c_size_t)]

extern crate alloc;

pub mod data;
pub mod libc;
pub mod peripheral;

pub mod characteristic;
mod discovery;
pub mod error;
mod nimble_sys;
mod peripheral_operation;
pub mod service;

use alloc::boxed::Box;
use alloc::slice;
use alloc::vec::Vec;

pub use uuid;

use crate::data::{BleGapDiscParams, RawAdvertisement};
// Re-export public types
pub use crate::data::Advertisement;
use crate::error::{Error, InternalError, ScanError, ScanResult};
use crate::nimble_sys::bindings::{
    BLE_HS_FOREVER, ble_transport_to_hs_acl_impl, ble_transport_to_hs_evt_impl,
};
use crate::nimble_sys::{
    bindings::{
        BLE_GAP_EVENT_DISC, BLE_GAP_EVENT_DISC_COMPLETE, BLE_GAP_EVENT_EXT_DISC, BLE_HS_EAGAIN,
        BLE_HS_EINVAL, MYNEWT_VAL_BLE_TRANSPORT_EVT_SIZE, ble_gap_disc_desc, ble_gap_event,
        ble_hci_cmd, ble_hci_ev, ble_hs_cfg, ble_transport_alloc_evt, ble_transport_free, os_mbuf,
        os_mbuf_append, os_mbuf_free_chain, os_msys_get_pkthdr,
    },
    ble_gap_disc, ble_gap_disc_cancel, ble_hs_adv_parse_fields, ble_hs_id_copy_addr,
    ble_hs_id_infer_auto, nimble_port_init, nimble_port_run,
};

use core::ffi::{c_int, c_void};
use core::task::Poll;

use embassy_futures::yield_now;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::waitqueue::AtomicWaker;
use embassy_sync::{
    blocking_mutex::raw::RawMutex,
    channel::Channel,
    pubsub::{PubSubChannel, Subscriber},
};
use esp_radio::ble::controller::BleConnector;
use portable_atomic::{AtomicBool, AtomicU8, Ordering};

// ── Host sync ────────────────────────────────────────────────────────────────

/// Flag that gates the use of the host API only after it came in sync with the controller
static HOST_CONTROLLER_SYNCED: AtomicBool = AtomicBool::new(false);
static SYNC_WAKER: AtomicWaker = AtomicWaker::new();
static OWN_ADDR_TYPE: AtomicU8 = AtomicU8::new(0);

/// Wait until the BLE host and controller are synchronised.
///
/// Must be called (and awaited) before using any scanner or connection APIs.
/// Returns immediately if already synced.
pub async fn wait_for_sync() {
    if !HOST_CONTROLLER_SYNCED.load(Ordering::Acquire) {
        SyncFuture.await;
    }
}

#[must_use = "futures do nothing unless you `.await` or poll them"]
struct SyncFuture;

impl core::future::Future for SyncFuture {
    type Output = ();

    fn poll(
        self: core::pin::Pin<&mut Self>,
        cx: &mut core::task::Context<'_>,
    ) -> Poll<Self::Output> {
        SYNC_WAKER.register(cx.waker());

        if HOST_CONTROLLER_SYNCED.load(Ordering::Acquire) {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    }
}

// ── Scanner ──────────────────────────────────────────────────────────────────

/// Default advertisement PubSub capacity.
const ADV_PUBSUB_CAP: usize = 32;

/// BLE advertisement scanner.
///
/// Wraps NimBLE's GAP discovery API with async Rust ergonomics. Call
/// [`Scanner::new()`] after [`wait_for_sync()`], then use
/// [`start_scan()`](Scanner::start_scan) / [`stop_scan()`](Scanner::stop_scan)
/// to control scanning, and [`subscribe()`](Scanner::subscribe) to receive
/// advertisements.
///
/// No background task needed — `ble_gap_disc` and `ble_gap_disc_cancel` are
/// called directly. The NimBLE host task delivers advertisements via the GAP
/// callback, which publishes into an internal `PubSubChannel`.
pub struct Scanner<M: RawMutex + 'static = CriticalSectionRawMutex> {
    inner: &'static ScannerInner<M>,
    scanning: bool,
    params: BleGapDiscParams,
}

struct ScannerInner<M: RawMutex + 'static> {
    adv_pub: PubSubChannel<M, RawAdvertisement, ADV_PUBSUB_CAP, 4, 1>,
}

impl<M: RawMutex + 'static> Scanner<M> {
    /// Create a new scanner. Call after [`wait_for_sync()`].
    ///
    /// Does not start scanning — call [`start_scan()`](Self::start_scan) to begin.
    pub fn new() -> Self {
        let inner: &'static ScannerInner<M> = Box::leak(Box::new(ScannerInner {
            adv_pub: PubSubChannel::new(),
        }));

        Self {
            inner,
            scanning: false,
            // Default: passive scanning, 50 ms window every 160 ms (~31% duty cycle).
            // Leaves radio time for WiFi coexistence. Units are 0.625 ms.
            params: BleGapDiscParams::new(0, 256, 80, false, true, false),
        }
    }

    /// Start BLE scanning.
    ///
    /// Pass `Some(params)` to configure scan interval, window, passive mode, etc.
    /// Pass `None` to use the current parameters (or defaults on first call).
    ///
    /// Always (re)starts scanning: cancels any in-progress scan first so that
    /// new parameters take effect and stale state from a NimBLE-internal stop
    /// (BLE_GAP_EVENT_DISC_COMPLETE) is recovered from.
    pub fn start_scan(&mut self, params: Option<BleGapDiscParams>) -> ScanResult<()> {
        // Cancel any active or stale scan; ignore the error if already stopped.
        let _ = ble_gap_disc_cancel();
        self.scanning = false;

        if let Some(p) = params {
            self.params = p;
        }

        let own_addr_type = OWN_ADDR_TYPE.load(Ordering::SeqCst);
        let cb_arg = self.inner as *const ScannerInner<M> as *mut c_void;

        ble_gap_disc(
            own_addr_type,
            BLE_HS_FOREVER as _,
            &self.params,
            Some(scan_event_handler::<M>),
            cb_arg,
        )
        .map_err(ScanError::GapDiscFailed)?;

        self.scanning = true;
        Ok(())
    }

    /// Stop BLE scanning.
    ///
    /// Idempotent — does nothing if not scanning.
    pub fn stop_scan(&mut self) -> ScanResult<()> {
        if !self.scanning {
            return Ok(());
        }

        ble_gap_disc_cancel().map_err(ScanError::GapDiscCancelFailed)?;
        self.scanning = false;
        Ok(())
    }

    /// Whether the scanner is currently active.
    pub fn is_scanning(&self) -> bool {
        self.scanning
    }

    /// Subscribe to received advertisements.
    ///
    /// Returns a `Subscriber` — call `next_message().await` in a loop to
    /// receive advertisements. Multiple subscribers are supported (up to 4).
    pub fn subscribe(
        &self,
    ) -> core::result::Result<
        Subscriber<'_, M, RawAdvertisement, ADV_PUBSUB_CAP, 4, 1>,
        InternalError,
    > {
        self.inner
            .adv_pub
            .subscriber()
            .map_err(|_| InternalError::ChannelClosed)
    }
}

/// GAP scan callback — publishes advertisements into the Scanner's PubSubChannel.
extern "C" fn scan_event_handler<M: RawMutex + 'static>(
    event: *mut ble_gap_event,
    arg: *mut c_void,
) -> c_int {
    if event.is_null() || arg.is_null() {
        panic!("scan_event_handler received null pointer: event={event:p}, arg={arg:p}");
    }

    let inner = unsafe { &*(arg as *const ScannerInner<M>) };
    let event = unsafe { *event };

    match event.type_ as u32 {
        BLE_GAP_EVENT_DISC | BLE_GAP_EVENT_EXT_DISC => {
            let disc: &ble_gap_disc_desc = unsafe { &event.__bindgen_anon_1.disc };
            match ble_hs_adv_parse_fields(disc) {
                Ok(_fields) => {
                    let data =
                        unsafe { slice::from_raw_parts(disc.data, disc.length_data as usize) };
                    let adv = RawAdvertisement::new(
                        disc.addr.into(),
                        disc.rssi,
                        heapless::Vec::from_slice(data)
                            .expect("Unable to create slice from advertisement data"),
                    );
                    match inner.adv_pub.publisher() {
                        Ok(p) => p.publish_immediate(adv),
                        Err(_) => log::warn!(
                            "[scanner] adv_pub publisher unavailable (too many subscribers?)"
                        ),
                    }
                }
                Err(e) => {
                    log::warn!("[scanner] adv parse failed (rc={}), skipping", e);
                }
            }
        }
        BLE_GAP_EVENT_DISC_COMPLETE => {
            log::info!("[scanner] NimBLE stopped scanning (DISC_COMPLETE)");
        }
        _ => {}
    }

    0
}

// ── HCI Transport ────────────────────────────────────────────────────────────

static HOST_2_CONTROLLER_QUEUE: Channel<CriticalSectionRawMutex, Vec<u8>, 20> = Channel::new();

const MAX_CMD_PARAMS: usize = 255;

const H4_CMD: u8 = 0x01;
const H4_ACL: u8 = 0x02;

pub struct HostTransport {
    controller: BleConnector<'static>,
}

impl HostTransport {
    pub fn new(controller_connector: BleConnector<'static>) -> Self {
        let new_host = Self {
            controller: controller_connector,
        };

        new_host.init();
        new_host
    }

    fn init(&self) {
        nimble_port_init();
        unsafe {
            ble_hs_cfg.sync_cb = Some(on_sync);
            ble_hs_cfg.reset_cb = Some(on_reset);
        }
    }
}

pub async fn transport_task_tx() {
    loop {
        let h2c_bytes = HOST_2_CONTROLLER_QUEUE.receive().await;
        log::trace!("[H2C] Forward {:02x?}", &h2c_bytes);
        esp_radio::ble::npl::send_hci(&h2c_bytes);
        log::trace!("[H2C] Forward done");
    }
}

pub async fn transport_task_rx(mut ble_host: HostTransport) {
    let mut buf = [0; 512];
    loop {
        log::trace!("[C2H] waiting for HCI ready");
        let read = ble_host.controller.read_async(&mut buf).await.unwrap();

        if read == 0 {
            continue;
        }

        let packet_bytes = &buf[..read];

        log::trace!("[C2H] Incoming packet {:02x?}", &packet_bytes);

        #[repr(u8)]
        #[derive(Debug, Default, num_enum::FromPrimitive)]
        enum PacketType {
            #[default]
            Invalid = 0xff,
            Acl = 0x02,
            Event = 0x04,
        }

        let packet_type = PacketType::from(packet_bytes[0]);

        match packet_type {
            PacketType::Acl => {
                // H4 ACL header: type(1) + handle(2) + len(2)
                if packet_bytes.len() < 1 + 2 + 2 {
                    panic!("short ACL packet");
                }

                let data_len = u16::from_le_bytes([packet_bytes[3], packet_bytes[4]]) as usize;
                let expected = 1 + 2 + 2 + data_len;
                if packet_bytes.len() < expected {
                    panic!(
                        "truncated ACL packet: got {}, expected {}",
                        packet_bytes.len(),
                        expected
                    );
                }

                // Build buffer with 4-byte HCI ACL header + payload
                let mut hdr = [0u8; 4];
                hdr[0..2].copy_from_slice(&packet_bytes[1..3]);
                hdr[2..4].copy_from_slice(&packet_bytes[3..5]);

                // Allocate an mbuf with pkthdr
                let om = loop {
                    let om = unsafe { os_msys_get_pkthdr(0, 0) };
                    if om.is_null() {
                        yield_now().await;
                        continue;
                    }
                    break om;
                };

                // Append HCI ACL header and payload
                let rc = unsafe { os_mbuf_append(om, hdr.as_ptr().cast(), hdr.len() as u16) };
                if rc != 0 {
                    unsafe { os_mbuf_free_chain(om) };
                    panic!("os_mbuf_append hdr failed");
                }

                let payload = &packet_bytes[5..5 + data_len];
                let rc =
                    unsafe { os_mbuf_append(om, payload.as_ptr().cast(), payload.len() as u16) };
                if rc != 0 {
                    unsafe { os_mbuf_free_chain(om) };
                    panic!("os_mbuf_append payload failed");
                }

                // Deliver to host
                let rc = unsafe { ble_transport_to_hs_acl_impl(om) };
                if rc != 0 {
                    unsafe { os_mbuf_free_chain(om) };
                    panic!("ble_transport_to_hs_acl_impl failed");
                }
            }
            PacketType::Event => {
                const PAYLOAD_OFFSET: usize = 3;
                let payload_len = packet_bytes[2] as usize;

                // First of all let's make sure that we have enough space for our data, by checking
                // against NimBLE's config
                if payload_len > MYNEWT_VAL_BLE_TRANSPORT_EVT_SIZE as usize - 2 {
                    panic!(
                        "Event data too long. MYNEWT_VAL_BLE_TRANSPORT_EVT_SIZE={}, payload_len={}",
                        MYNEWT_VAL_BLE_TRANSPORT_EVT_SIZE, payload_len
                    );
                }

                // Try to allocate memory for the event
                let hci_ev = loop {
                    unsafe {
                        // TODO: Make safe wrapper in nimble_sys/transport
                        let ev = ble_transport_alloc_evt(0);
                        if ev.is_null() {
                            yield_now().await;
                        } else {
                            break ev as *mut ble_hci_ev;
                        }
                    };
                };

                unsafe {
                    (*hci_ev).opcode = packet_bytes[1];
                    (*hci_ev).length = payload_len as u8;
                    (*hci_ev)
                        .data
                        .as_mut_slice(payload_len)
                        .copy_from_slice(&packet_bytes[PAYLOAD_OFFSET..]);
                }

                unsafe {
                    if ble_transport_to_hs_evt_impl(hci_ev as *mut c_void) != 0 {
                        ble_transport_free(hci_ev as *mut _);
                        panic!("Failed to send event to Host");
                    }
                }
            }
            PacketType::Invalid => {
                todo!("Packet type not handled yet: {:02x}", packet_bytes[0])
            }
        }
    }
}

pub extern "C" fn host_task(_: *mut c_void) {
    log::info!("[TASK] Running BLE Host");
    nimble_port_run();
}

// ── NimBLE C FFI callbacks ───────────────────────────────────────────────────

#[unsafe(no_mangle)]
pub unsafe extern "C" fn ble_transport_ll_init() {
    // No-Op because esp-radio does it for us
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn ble_transport_to_ll_cmd_impl(buf: *mut c_void) -> c_int {
    if buf.is_null() {
        return BLE_HS_EINVAL as _;
    }

    let cmd_pkt = buf as *mut ble_hci_cmd;
    let opcode = unsafe { (*cmd_pkt).opcode };
    let pkt_len = unsafe { (*cmd_pkt).length as usize };
    if pkt_len > MAX_CMD_PARAMS {
        return BLE_HS_EINVAL as _;
    }

    // Parameters live immediately after the header (flexible array member).
    let params = unsafe { core::slice::from_raw_parts((*cmd_pkt).data.as_ptr(), pkt_len) };

    // Build H4 frame: type + opcode(le) + len + params
    let mut cmd_packet = Vec::new();
    cmd_packet.push(H4_CMD);
    cmd_packet.extend_from_slice(&opcode.to_le_bytes());
    cmd_packet.push(u8::try_from(pkt_len).unwrap());
    cmd_packet.extend_from_slice(params);

    unsafe { ble_transport_free(buf) };

    match HOST_2_CONTROLLER_QUEUE.try_send(cmd_packet) {
        Ok(()) => 0,
        Err(_) => BLE_HS_EAGAIN as _,
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn ble_transport_to_ll_acl_impl(om: *mut os_mbuf) -> c_int {
    if om.is_null() {
        return BLE_HS_EINVAL as _;
    }

    // Compute total length of the mbuf chain.
    let mut total_len: usize = 0;
    {
        let mut cur = om;
        while !cur.is_null() {
            total_len += (*cur).om_len as usize;
            cur = (*cur).om_next.sle_next;
        }
    }

    // Must include at least the HCI ACL header (4 bytes).
    if total_len < 4 {
        os_mbuf_free_chain(om);
        return BLE_HS_EINVAL as _;
    }

    // Build H4 frame: type + (HCI ACL header + payload)
    let mut packet = Vec::with_capacity(1 + total_len);
    packet.push(H4_ACL);

    // Copy mbuf chain into packet
    {
        let mut cur = om;
        while !cur.is_null() {
            let seg_len = (*cur).om_len as usize;
            if seg_len != 0 {
                let seg = core::slice::from_raw_parts((*cur).om_data as *const u8, seg_len);
                packet.extend_from_slice(seg);
            }
            cur = (*cur).om_next.sle_next;
        }
    }

    // Free the chain now that we've copied it out
    os_mbuf_free_chain(om);

    // Send to controller transport
    match HOST_2_CONTROLLER_QUEUE.try_send(packet) {
        Ok(()) => 0,
        Err(_) => BLE_HS_EAGAIN as _,
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn ble_transport_to_ll_iso_impl(_om: *mut os_mbuf) -> c_int {
    todo!()
}

/// NimBLE's callback which indicates the Host and the Controller are now synced
#[unsafe(no_mangle)]
unsafe extern "C" fn on_sync() {
    let addr_type = ble_hs_id_infer_auto(false).expect("Failed to infer BLE address type");
    OWN_ADDR_TYPE.store(addr_type, Ordering::Release);

    let addr = ble_hs_id_copy_addr(addr_type).expect("Failed to set BLE address");

    log::info!(
        "BLE address: {:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X}, Type: {addr_type}",
        addr[5],
        addr[4],
        addr[3],
        addr[2],
        addr[1],
        addr[0]
    );

    // Signal that we can now use the host API
    HOST_CONTROLLER_SYNCED.store(true, Ordering::Release);
    SYNC_WAKER.wake();
}

/// NimBLE's callback which indicates a reset of the state
#[unsafe(no_mangle)]
unsafe extern "C" fn on_reset(reason: c_int) {
    log::trace!("on_reset: reason {:?}", reason);
    if reason == 19 {
        panic!("Host lost sync with controller");
    }
}
