#![no_std]
#![feature(c_size_t)]

extern crate alloc;

pub mod data;
pub mod libc;
pub mod peripheral;

mod characteristic;
mod discovery;
mod error;
mod nimble_sys;
mod peripheral_operation;
mod service;

use alloc::boxed::Box;
use alloc::slice;
use alloc::sync::Arc;
use alloc::vec::Vec;

pub use uuid;

use crate::data::{Advertisement, BleGapDiscParams, RawAdvertisement};
use crate::error::{Error, Result};
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
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex as DefaultRawMutex;
use embassy_sync::{
    blocking_mutex::raw::{CriticalSectionRawMutex, RawMutex},
    channel::Channel,
    pubsub::{PubSubChannel, Subscriber},
    signal::Signal,
};
use embassy_sync::waitqueue::AtomicWaker;
use esp_radio::ble::controller::BleConnector;
use portable_atomic::{AtomicBool, AtomicU8, Ordering};

/// Flag that gates the use of the host API only after it came in sync with the controller
static HOST_CONTROLLER_SYNCED: AtomicBool = AtomicBool::new(false);
static SYNC_WAKER: AtomicWaker = AtomicWaker::new();
static OWN_ADDR_TYPE: AtomicU8 = AtomicU8::new(0);

#[must_use = "futures do nothing unless you `.await` or poll them"]
pub struct SyncFuture;

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

static HOST_2_CONTROLLER_QUEUE: Channel<CriticalSectionRawMutex, Vec<u8>, 20> = Channel::new();

const MAX_CMD_PARAMS: usize = 255;

const H4_CMD: u8 = 0x01;
const H4_ACL: u8 = 0x02;

/// Scan control command
struct ScanCommand<M: RawMutex> {
    pause: bool,
    done: Arc<Signal<M, Result>>,
}

pub struct ScannerControl<M: RawMutex + 'static = DefaultRawMutex> {
    cmd_tx: &'static Channel<M, ScanCommand<M>, 4>,
    paused: Arc<AtomicBool>,
}

// Manual Clone avoids requiring M: Clone.
impl<M: RawMutex + 'static> Clone for ScannerControl<M> {
    fn clone(&self) -> Self {
        Self {
            cmd_tx: self.cmd_tx,
            paused: self.paused.clone(),
        }
    }
}

impl<M: RawMutex + 'static> ScannerControl<M> {
    fn new(cmd_tx: &'static Channel<M, ScanCommand<M>, 4>) -> Self {
        Self {
            cmd_tx,
            paused: Arc::new(AtomicBool::new(true)),
        }
    }

    pub async fn pause(&self) -> Result<()> {
        if self.paused.load(Ordering::SeqCst) {
            return Ok(());
        }

        let done = Arc::new(Signal::<M, Result>::new());
        self.cmd_tx
            .send(ScanCommand {
                pause: true,
                done: done.clone(),
            })
            .await;

        match done.wait().await {
            Ok(()) => {
                self.paused.store(true, Ordering::SeqCst);
                Ok(())
            }
            Err(e) => {
                self.paused.store(false, Ordering::SeqCst);
                Err(e)
            }
        }
    }

    pub async fn resume(&self) -> Result<()> {
        if !self.paused.load(Ordering::SeqCst) {
            return Ok(());
        }

        let done = Arc::new(Signal::<M, Result>::new());
        self.cmd_tx
            .send(ScanCommand {
                pause: false,
                done: done.clone(),
            })
            .await;

        match done.wait().await {
            Ok(()) => {
                self.paused.store(false, Ordering::SeqCst);
                Ok(())
            }
            Err(e) => {
                self.paused.store(true, Ordering::SeqCst);
                Err(e)
            }
        }
    }
}

/// Inner shared state (leaked to 'static).
struct BleHostInner<M: RawMutex + 'static> {
    scan_cmd: Channel<M, ScanCommand<M>, 4>,
    adv_pub: PubSubChannel<M, RawAdvertisement, 32, 1, 1>,
}

pub struct ScannerTask<M: RawMutex + 'static = DefaultRawMutex> {
    inner: &'static BleHostInner<M>,
}

pub struct BleHost<M: RawMutex + 'static = DefaultRawMutex> {
    inner: &'static BleHostInner<M>,
    scanner_control: ScannerControl<M>,
}

impl<M: RawMutex + 'static> BleHost<M> {
    /// Create host handle + scanner task handle.
    /// You must spawn `ScannerTask::run()` on the executor.
    pub async fn new() -> (Self, ScannerTask<M>) {
        let inner: &'static BleHostInner<M> = Box::leak(Box::new(BleHostInner {
            scan_cmd: Channel::new(),
            adv_pub: PubSubChannel::new(),
        }));

        if !HOST_CONTROLLER_SYNCED.load(Ordering::Acquire) {
            SyncFuture.await;
        }

        let host = Self {
            inner,
            scanner_control: ScannerControl::new(&inner.scan_cmd),
        };

        let scanner = ScannerTask { inner };
        (host, scanner)
    }

    pub fn scanner_control(&self) -> ScannerControl<M> {
        self.scanner_control.clone()
    }

    /// Subscribe to advertisements. Consumer should loop `next_message().await`.
    pub fn subscribe_advertisements(
        &self,
    ) -> core::result::Result<Subscriber<'_, M, RawAdvertisement, 32, 1, 1>, Error> {
        self.inner
            .adv_pub
            .subscriber()
            .map_err(|_| Error::ResultChannelClosed)
    }
}

impl<M: RawMutex + 'static> ScannerTask<M> {
    pub async fn run(self) -> ! {
        // TODO: define sane defaults
        let disc_params = BleGapDiscParams::new(0, 0, 0, false, true, false);
        let mut is_paused = true;

        loop {
            let cmd = self.inner.scan_cmd.receive().await;

            if cmd.pause {
                if !is_paused {
                    match ble_gap_disc_cancel() {
                        Ok(_) => cmd.done.signal(Ok(())),
                        Err(_) => cmd.done.signal(Err(Error::ScannerControlFailedToPause)),
                    }
                    is_paused = true;
                } else {
                    cmd.done.signal(Ok(()));
                }
            } else {
                if is_paused {
                    let own_addr_type = OWN_ADDR_TYPE.load(Ordering::SeqCst);
                    let param = self.inner as *const BleHostInner<M> as *mut c_void;

                    match ble_gap_disc(
                        own_addr_type,
                        BLE_HS_FOREVER as _,
                        &disc_params,
                        Some(ble_gap_event_handler::<M>),
                        param,
                    ) {
                        Ok(_) => cmd.done.signal(Ok(())),
                        Err(_) => cmd.done.signal(Err(Error::ScannerControlFailedToResume)),
                    }
                    is_paused = false;
                } else {
                    cmd.done.signal(Ok(()));
                }
            }
        }
    }
}

/// GAP scan callback (generic over RawMutex so it can publish into the correct channel type).
extern "C" fn ble_gap_event_handler<M: RawMutex + 'static>(
    event: *mut ble_gap_event,
    arg: *mut c_void,
) -> c_int {
    if event.is_null() || arg.is_null() {
        panic!("ble_gap_event_handler received null pointer: event={event:p}, arg={arg:p}");
    }

    let inner = unsafe { &*(arg as *const BleHostInner<M>) };
    let event = unsafe { *event };

    match event.type_ as u32 {
        BLE_GAP_EVENT_DISC | BLE_GAP_EVENT_EXT_DISC => {
            let disc: &ble_gap_disc_desc = unsafe { &event.__bindgen_anon_1.disc };
            if let Ok(fields) = ble_hs_adv_parse_fields(disc) {
                let data = unsafe { slice::from_raw_parts(disc.data, disc.length_data as usize) };
                let adv = RawAdvertisement::new(
                    disc.addr.into(),
                    disc.rssi,
                    heapless::Vec::from_slice(data)
                        .expect("Unable to create slice from advertisement data"),
                );
                if let Ok(p) = inner.adv_pub.publisher() {
                    p.publish_immediate(adv);
                }
            }
        }
        BLE_GAP_EVENT_DISC_COMPLETE => {
            log::info!("Scanning stopped");
        }
        _ => {}
    }

    0
}

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
                let rc = unsafe {
                    os_mbuf_append(om, payload.as_ptr().cast(), payload.len() as u16)
                };
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
    // (Function names may differ slightly in your bindings; adjust if needed.)
    let mut total_len: usize = 0;
    {
        let mut cur = om;
        while !cur.is_null() {
            // os_mbuf fields: om_len is length of this segment, om_next is next segment
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
