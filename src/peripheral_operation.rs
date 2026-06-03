use alloc::sync::Arc;
use core::cell::RefCell;
use core::sync::atomic::{AtomicBool, Ordering};

use embassy_sync::{
    blocking_mutex::{Mutex, raw::RawMutex},
    signal::Signal,
};

use crate::{data::ConnectionHandle, error::GattResult};

/// Represents an operation on a peripheral device.
///
/// `M` selects the mutex backend (e.g. critical-section, thread-mode, etc.).
pub struct PeripheralOperation<C, M: RawMutex> {
    conn_handle: ConnectionHandle,

    // One-shot completion signaling.
    finished: Arc<Signal<M, GattResult>>,
    finished_sent: AtomicBool,

    // Shared operation context. `RefCell` provides safe interior mutability;
    // the surrounding `Mutex` serialises the callback (host task) against the
    // awaiter, so `borrow_mut()` never overlaps in normal operation.
    context: Arc<Mutex<M, RefCell<C>>>,
}

impl<C, M: RawMutex> PeripheralOperation<C, M> {
    fn new(
        conn_handle: ConnectionHandle,
        finished: Arc<Signal<M, GattResult>>,
        context: C,
    ) -> Self {
        Self {
            conn_handle,
            finished,
            finished_sent: AtomicBool::new(false),
            context: Arc::new(Mutex::new(RefCell::new(context))),
        }
    }

    /// Completes the operation by signaling the result.
    ///
    /// Safe to call from synchronous contexts (e.g. event handlers), provided the chosen `M`
    /// is appropriate for that context.
    pub fn send_finished(&self, result: GattResult) {
        // Ensure "oneshot" semantics.
        if self.finished_sent.swap(true, Ordering::AcqRel) {
            log::error!("Finished already sent");
            return;
        }

        self.finished.signal(result);
    }

    /// Returns the connection handle associated with this operation.
    pub fn conn_handle(&self) -> u16 {
        self.conn_handle
    }

    /// Takes ownership of the context, returning it only if this is the last `Arc`.
    pub fn take_context(self) -> Option<C> {
        Arc::try_unwrap(self.context)
            .ok()
            .map(|m| m.into_inner().into_inner())
    }

    /// Returns a reference to the context mutex.
    pub fn context(&self) -> &Mutex<M, RefCell<C>> {
        &self.context
    }
}

/// Represents a handle to a peripheral operation.
pub struct PeripheralOperationHandle<M: RawMutex> {
    finished: Arc<Signal<M, GattResult>>,
}

impl<M: RawMutex> PeripheralOperationHandle<M> {
    fn new(finished: Arc<Signal<M, GattResult>>) -> Self {
        Self { finished }
    }

    /// Awaits the completion of the operation.
    pub async fn join(self) -> GattResult<()> {
        self.finished.wait().await
    }
}

/// Creates a new peripheral operation and its handle.
pub fn peripheral_operation<C, M: RawMutex>(
    conn_handle: ConnectionHandle,
    context: C,
) -> (PeripheralOperation<C, M>, PeripheralOperationHandle<M>) {
    let finished = Arc::new(Signal::<M, GattResult>::new());

    (
        PeripheralOperation::new(conn_handle, finished.clone(), context),
        PeripheralOperationHandle::new(finished),
    )
}
