use alloc::string::String;

use thiserror::Error;

use crate::nimble_sys::NimbleError;

/// Represents errors that can occur in the rimble crate.
#[derive(Debug, Error)]
pub enum Error {
    #[error("Nimble synchronization error: {0}")]
    Sync(#[source] NimbleError),
    #[error("Nimble set MTU error: {0}")]
    SetMtu(#[source] NimbleError),
    #[error("Nimble exchange MTU error: {0}")]
    ExchangeMtu(#[source] NimbleError),
    #[error("Nimble initialized already")]
    AlreadyInitialized,
    #[error("Unable to join HCI socket thread")]
    JoinHciSocketThread,
    #[error("Unable to join BLE host thread")]
    JoinBleHostThread,
    #[error("ScannerControl: Failed to pause scanner")]
    ScannerControlFailedToPause,
    #[error("ScannerControl: Failed to resume scanner")]
    ScannerControlFailedToResume,
    #[error("Read attribute error: {0}")]
    ReadAttribute(#[source] NimbleError),
    #[error("Write attribute error: {0}")]
    WriteAttribute(#[source] NimbleError),
    #[error("Peripheral connection error: {0}")]
    Connect(#[source] NimbleError),
    #[error("Peripheral connect timeout")]
    ConnectTimeout,
    #[error("Peripheral disconnected while waiting for operation")]
    DisconnectedWhileOperation,
    #[error(
        "Peripheral has connection handle {current_handle} already, cannot set new handle {new_handle}"
    )]
    AlreadyConnected {
        current_handle: u16,
        new_handle: u16,
    },
    #[error("Nimble service discovery error: {0}")]
    ServiceDiscovery(#[source] NimbleError),
    #[error("No services discovered")]
    NoServicesDiscovered,
    #[error("Nimble descriptor discovery error: {0}")]
    DescriptorDiscovery(#[source] NimbleError),
    #[error("No descriptors discovered")]
    NoDescriptorsDiscovered,
    #[error("Nimble characteristic discovery error: {0}")]
    CharacteristicDiscovery(#[source] NimbleError),
    #[error("No characteristics discovered")]
    NoCharacteristicsDiscovered,
    #[error("Nimble disconnection error: {0}")]
    Disconnect(#[source] NimbleError),
    #[error("Peripheral not connected")]
    NotConnected,
    #[error("Nimble threads not started")]
    NimbleThreadsNotStarted,
    #[error("Peripheral error: {0}")]
    Peripheral(String),
    #[error("Subscription error: {0}")]
    Subscription(String),
    #[error("Nimble read characteristic error: {0}")]
    ReadCharacteristic(String),
    #[error("Nimble write characteristic error: {0}")]
    WriteCharacteristic(String),
    #[error("Invalid argument: {0}")]
    InvalidArgument(String),
    #[error("Nimble scan error: {0}")]
    Scan(String),
    #[error("Nimble host error: {0}")]
    Host(String),
    #[error("Async operation error: {0}")]
    AsyncOperation(String),
    #[error("BLE address conversion error")]
    BleAddrConversion,
    #[error("UUID conversion error: {0}")]
    UuidConversion(String),
    #[error("Result channel closed")]
    ResultChannelClosed,
    #[error("Control channel closed")]
    ControlChannelClosed,
    #[error("Resource lock failed: {0}")]
    ResourceLockFailed(String),
}

/// Result type for the rimble crate, defaulting to `()`.
pub type Result<T = ()> = core::result::Result<T, Error>;
