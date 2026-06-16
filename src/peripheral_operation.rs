use alloc::{boxed::Box, sync::Arc};
use core::cell::RefCell;
use core::sync::atomic::{AtomicBool, Ordering};

use embassy_sync::{
    blocking_mutex::{Mutex, raw::RawMutex},
    signal::Signal,
};

use crate::{data::ConnectionHandle, error::GattResult};

/// Shared state between the in-flight NimBLE callback and the awaiting async caller.
///
/// Heap-allocated so that `Box::into_raw` can hand a stable pointer to NimBLE. The
/// terminal callback must reconstitute and drop the `Box` via `Box::from_raw`.
pub struct PeripheralOperation<C, M: RawMutex> {
    conn_handle: ConnectionHandle,
    finished: Arc<Signal<M, GattResult>>,
    finished_sent: AtomicBool,
    context: Arc<Mutex<M, RefCell<C>>>,
}

impl<C, M: RawMutex> PeripheralOperation<C, M> {
    /// Completes the operation by signaling the result.
    ///
    /// Safe to call from synchronous contexts (e.g. event handlers), provided the chosen `M`
    /// is appropriate for that context.
    pub fn send_finished(&self, result: GattResult) {
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

    /// Returns a reference to the context mutex.
    pub fn context(&self) -> &Mutex<M, RefCell<C>> {
        &self.context
    }
}

/// Handle held by the async caller; awaits completion and retrieves the context.
///
/// Holds a clone of the context `Arc` so that after the callback drops its
/// `Box<PeripheralOperation>` (decrementing the refcount to 1), `take_context`
/// can reclaim exclusive ownership.
pub struct PeripheralOperationHandle<C, M: RawMutex> {
    finished: Arc<Signal<M, GattResult>>,
    context: Arc<Mutex<M, RefCell<C>>>,
}

impl<C, M: RawMutex> PeripheralOperationHandle<C, M> {
    /// Awaits the completion of the operation.
    pub async fn join(&self) -> GattResult {
        self.finished.wait().await
    }

    /// Retrieves the context after the operation completes.
    ///
    /// Returns `None` if the callback's `Box` has not yet been dropped (the Arc
    /// refcount is still > 1). In normal operation this is always `Some` after
    /// `join().await` returns.
    pub fn take_context(self) -> Option<C> {
        Arc::try_unwrap(self.context)
            .ok()
            .map(|m| m.into_inner().into_inner())
    }
}

/// Creates a new peripheral operation and its handle.
///
/// Returns a heap-allocated operation for safe FFI lifetime handoff and a handle
/// for awaiting the result. Pass the raw pointer to NimBLE via `Box::into_raw`;
/// the terminal callback must reconstitute and drop it via `Box::from_raw`.
pub fn peripheral_operation<C, M: RawMutex>(
    conn_handle: ConnectionHandle,
    context: C,
) -> (Box<PeripheralOperation<C, M>>, PeripheralOperationHandle<C, M>) {
    let finished = Arc::new(Signal::<M, GattResult>::new());
    let context = Arc::new(Mutex::new(RefCell::new(context)));

    (
        Box::new(PeripheralOperation {
            conn_handle,
            finished: finished.clone(),
            finished_sent: AtomicBool::new(false),
            context: context.clone(),
        }),
        PeripheralOperationHandle { finished, context },
    )
}
