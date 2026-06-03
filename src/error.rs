use alloc::string::String;

use crate::nimble_sys::NimbleError;

// ── Per-class errors ─────────────────────────────────────────────────────────

/// Errors from BLE scanning (GAP discovery).
#[derive(Debug)]
pub enum ScanError {
    /// `ble_gap_disc()` failed.
    GapDiscFailed(NimbleError),
    /// `ble_gap_disc_cancel()` failed.
    GapDiscCancelFailed(NimbleError),
}

/// Errors from BLE connection lifecycle.
#[derive(Debug)]
pub enum ConnectError {
    /// `ble_gap_connect()` failed.
    GapConnectFailed(NimbleError),
    /// Connection attempt timed out.
    Timeout,
    /// Already connected (existing handle + new handle).
    AlreadyConnected {
        current_handle: u16,
        new_handle: u16,
    },
    /// `ble_gap_terminate()` failed.
    DisconnectFailed(NimbleError),
    /// Connection dropped while an operation was pending.
    DisconnectedWhileOperation,
    /// Not connected (operation requires an active connection).
    NotConnected,
}

/// Errors from GATT operations (read, write, discover, subscribe).
#[derive(Debug)]
pub enum GattError {
    /// Not connected (operation requires an active connection).
    NotConnected,
    /// Attribute read failed at the NimBLE level.
    ReadFailed(NimbleError),
    /// Attribute write failed at the NimBLE level.
    WriteFailed(NimbleError),
    /// Read completed successfully but returned no data.
    NoData,
    /// MTU exchange failed.
    MtuExchangeFailed(NimbleError),
    /// ATT MTU is zero for the given connection.
    AttMtuZero(u16),
    /// Failed to allocate an mbuf for the write operation.
    MbufCreationFailed,
    /// Service discovery failed at the NimBLE level.
    ServiceDiscoveryFailed(NimbleError),
    /// No services found.
    NoServicesDiscovered,
    /// Characteristic discovery failed at the NimBLE level.
    CharacteristicDiscoveryFailed(NimbleError),
    /// No characteristics found.
    NoCharacteristicsDiscovered,
    /// Descriptor discovery failed at the NimBLE level.
    DescriptorDiscoveryFailed(NimbleError),
    /// No descriptors found.
    NoDescriptorsDiscovered,
    /// Connection dropped while a GATT operation was pending.
    DisconnectedWhileOperation,
    /// Data conversion error during a GATT operation (e.g. UUID parsing).
    Data(DataError),
}

impl From<DataError> for GattError {
    fn from(e: DataError) -> Self {
        Self::Data(e)
    }
}

/// Errors from data conversion (addresses, UUIDs, arguments).
#[derive(Debug)]
pub enum DataError {
    /// BLE address conversion failed.
    BleAddrConversion,
    /// UUID conversion failed.
    UuidConversion(String),
    /// Invalid argument.
    InvalidArgument(String),
}

/// Internal errors (channel/infrastructure issues).
#[derive(Debug)]
pub enum InternalError {
    /// A PubSub or Signal channel was closed unexpectedly.
    ChannelClosed,
}

// ── Top-level wrapper ────────────────────────────────────────────────────────

/// Top-level error type wrapping all per-class errors.
///
/// Use the specific error types (`ScanError`, `ConnectError`, `GattError`) when
/// you want precise matching. Use `Error` when you want a single catch-all type
/// with `?` propagation across different operations.
#[derive(Debug)]
pub enum Error {
    Scan(ScanError),
    Connect(ConnectError),
    Gatt(GattError),
    Data(DataError),
    Internal(InternalError),
}

// ── From impls for ? propagation ─────────────────────────────────────────────

impl From<ScanError> for Error {
    fn from(e: ScanError) -> Self {
        Self::Scan(e)
    }
}

impl From<ConnectError> for Error {
    fn from(e: ConnectError) -> Self {
        Self::Connect(e)
    }
}

impl From<GattError> for Error {
    fn from(e: GattError) -> Self {
        Self::Gatt(e)
    }
}

impl From<DataError> for Error {
    fn from(e: DataError) -> Self {
        Self::Data(e)
    }
}

impl From<InternalError> for Error {
    fn from(e: InternalError) -> Self {
        Self::Internal(e)
    }
}

// ── Result aliases ───────────────────────────────────────────────────────────

/// Result with the top-level [`Error`] type.
pub type Result<T = ()> = core::result::Result<T, Error>;

/// Result with [`ScanError`].
pub type ScanResult<T = ()> = core::result::Result<T, ScanError>;

/// Result with [`ConnectError`].
pub type ConnectResult<T = ()> = core::result::Result<T, ConnectError>;

/// Result with [`GattError`].
pub type GattResult<T = ()> = core::result::Result<T, GattError>;
