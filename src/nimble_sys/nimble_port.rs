use super::bindings;

/// Initializes the NimBLE port.
///
/// This function must be called before using any other NimBLE functionality.
pub fn nimble_port_init() {
    unsafe {
        bindings::nimble_port_init();
    }
}

/// Runs the NimBLE port event loop.
///
/// This function typically blocks and processes BLE events.
pub fn nimble_port_run() {
    unsafe {
        bindings::nimble_port_run();
    }
}
