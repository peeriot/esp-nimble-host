use super::bindings;

/// Allocates a buffer for an HCI event from the transport layer.
///
/// Returns `None` if no memory is currently available; the caller should
/// retry (e.g. yield and loop) until allocation succeeds.
pub fn transport_alloc_evt() -> Option<*mut bindings::ble_hci_ev> {
    let ev = unsafe { bindings::ble_transport_alloc_evt(0) };
    if ev.is_null() {
        None
    } else {
        Some(ev as *mut bindings::ble_hci_ev)
    }
}
