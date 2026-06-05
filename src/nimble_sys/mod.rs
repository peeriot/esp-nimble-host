#![allow(dead_code)]

mod att;
pub mod gap;
mod gattc;
mod gatts;
pub mod host;
pub mod nimble_port;

use thiserror::Error;

pub mod bindings {
    #![allow(
        non_camel_case_types,
        non_upper_case_globals,
        non_snake_case,
        unsafe_op_in_unsafe_fn,
        unused,
        dead_code,
        clippy::all
    )]

    include!(concat!(env!("OUT_DIR"), "/nimble_host_bindings.rs"));
}

pub(crate) use self::{att::*, gap::*, gattc::*, host::*, nimble_port::*};

#[derive(Debug, Error)]
pub enum NimbleError {
    #[error("Operation already in progress or completed.")]
    OperationInProgress,
    #[error("Operation cannot be performed until procedure completes.")]
    Busy,
    #[error("One or more arguments are invalid.")]
    InvalidArgument,
    #[error("The provided buffer is too small.")]
    MessageSize,
    #[error("No entry matching the specified criteria.")]
    NoEntry,
    #[error("Operation failed due to resource exhaustion.")]
    NoMemory,
    #[error("No open connection with the specified handle.")]
    NotConnected,
    #[error("Operation disabled at compile time.")]
    NotSupported,
    #[error("Application callback behaved unexpectedly.")]
    AppError,
    #[error("Command from peer is invalid.")]
    BadData,
    #[error("Mynewt OS error.")]
    OsError,
    #[error("Event from controller is invalid.")]
    ControllerError,
    #[error("Operation timed out.")]
    Timeout,
    #[error("Peer rejected a connection parameter update request.")]
    Reject,
    #[error("Unexpected failure; catch all.")]
    Unknown,
    #[error("Operation requires different role (e.g., central vs. peripheral).")]
    Role,
    #[error("HCI request timed out; controller unresponsive.")]
    TimeoutHci,
    #[error(
        "Controller failed to send event due to memory exhaustion (combined host-controller only)."
    )]
    NoMemEvt,
    #[error("Operation requires an identity address but none configured.")]
    NoAddr,
    #[error("Attempt to use the host before it is synced with controller.")]
    NotSynced,
    #[error("Insufficient authentication.")]
    Authen,
    #[error("Insufficient authorization.")]
    Author,
    #[error("Insufficient encryption level.")]
    Encrypt,
    #[error("Insufficient key size")]
    EncryptKeySize,
    #[error("Storage at capacity.")]
    StoreCap,
    #[error("Storage IO error.")]
    StoreFail,
    // ATT errors
    #[error("The attribute handle given was not valid on this server.")]
    AttInvalidHandle,
    #[error("The attribute cannot be read.")]
    AttReadNotPermitted,
    #[error("The attribute cannot be written.")]
    AttWriteNotPermitted,
    #[error("The attribute PDU was invalid.")]
    AttInvalidPdu,
    #[error("The attribute requires authentication before it can be read or written.")]
    AttInsufficientAuthen,
    #[error("Attribute server does not support the request received from the client.")]
    AttReqNotSupported,
    #[error("Offset specified was past the end of the attribute.")]
    AttInvalidOffset,
    #[error("The attribute requires authorization before it can be read or written.")]
    AttInsufficientAuthor,
    #[error("Too many prepare writes have been queued.")]
    AttPrepareQueueFull,
    #[error("No attribute found within the given attribute handle range.")]
    AttAttrNotFound,
    #[error("The attribute cannot be read or written using the Read Blob Request.")]
    AttAttrNotLong,
    #[error("The Encryption Key Size used for encrypting this link is insufficient.")]
    AttInsufficientKeySize,
    #[error("The attribute value length is invalid for the operation.")]
    AttInvalidAttrValueLen,
    #[error(
        "The attribute request has encountered an error that was unlikely, could not be completed as requested."
    )]
    AttUnlikely,
    #[error("The attribute requires encryption before it can be read or written.")]
    AttInsufficientEnc,
    #[error(
        "The attribute type is not a supported grouping attribute as defined by a higher layer specification."
    )]
    AttUnsupportedGroup,
    #[error("Insufficient Resources to complete the request.")]
    AttInsufficientRes,
    // HCI errors
    #[error("Unknown HCI Command")]
    HciUnknownCmd,
    #[error("Unknown Connection Identifier")]
    HciUnknownConnId,
    #[error("Connection Terminated By Local Host")]
    HciConnTermLocal,
    #[error("Connection Terminated By Remote")]
    HciConnTermRemote,
    #[error("Connection Failed to be Established.")]
    HciConnEstablishment,
    // Other errors
    #[error("Failed to get MTU for connection handle {0}: MTU is zero")]
    AttMtuZero(u16),
    #[error("Unable to create mbuf from flat data")]
    MbufCreationFailed,
    #[error("Unable to convert string to CString: {0}")]
    CStringConversionFailed(alloc::string::String),
    // Fallback for unmapped error codes
    #[error("Unknown error code: {0}")]
    Other(u32),
}

pub type NimbleResult<T = ()> = Result<T, NimbleError>;

pub fn return_code_to_result<T>(rc: u32, val: T) -> NimbleResult<T> {
    use NimbleError::*;
    if rc == 0 || rc == bindings::BLE_HS_EDONE {
        return Ok(val);
    }
    let err = if rc < bindings::BLE_HS_ERR_ATT_BASE {
        match rc {
            bindings::BLE_HS_EALREADY => OperationInProgress,
            bindings::BLE_HS_EBUSY => Busy,
            bindings::BLE_HS_EINVAL => InvalidArgument,
            bindings::BLE_HS_EMSGSIZE => MessageSize,
            bindings::BLE_HS_ENOENT => NoEntry,
            bindings::BLE_HS_ENOMEM => NoMemory,
            bindings::BLE_HS_ENOTCONN => NotConnected,
            bindings::BLE_HS_ENOTSUP => NotSupported,
            bindings::BLE_HS_EAPP => AppError,
            bindings::BLE_HS_EBADDATA => BadData,
            bindings::BLE_HS_EOS => OsError,
            bindings::BLE_HS_ECONTROLLER => ControllerError,
            bindings::BLE_HS_ETIMEOUT => Timeout,
            bindings::BLE_HS_EREJECT => Reject,
            bindings::BLE_HS_EUNKNOWN => Unknown,
            bindings::BLE_HS_EROLE => Role,
            bindings::BLE_HS_ETIMEOUT_HCI => TimeoutHci,
            bindings::BLE_HS_ENOMEM_EVT => NoMemEvt,
            bindings::BLE_HS_ENOADDR => NoAddr,
            bindings::BLE_HS_ENOTSYNCED => NotSynced,
            bindings::BLE_HS_EAUTHEN => Authen,
            bindings::BLE_HS_EAUTHOR => Author,
            bindings::BLE_HS_EENCRYPT => Encrypt,
            bindings::BLE_HS_EENCRYPT_KEY_SZ => EncryptKeySize,
            bindings::BLE_HS_ESTORE_CAP => StoreCap,
            bindings::BLE_HS_ESTORE_FAIL => StoreFail,
            _ => Other(rc),
        }
    } else if rc < bindings::BLE_HS_ERR_HCI_BASE {
        let rc_ = rc - bindings::BLE_HS_ERR_ATT_BASE;
        match rc_ {
            bindings::BLE_ATT_ERR_INVALID_HANDLE => AttInvalidHandle,
            bindings::BLE_ATT_ERR_READ_NOT_PERMITTED => AttReadNotPermitted,
            bindings::BLE_ATT_ERR_WRITE_NOT_PERMITTED => AttWriteNotPermitted,
            bindings::BLE_ATT_ERR_INVALID_PDU => AttInvalidPdu,
            bindings::BLE_ATT_ERR_INSUFFICIENT_AUTHEN => AttInsufficientAuthen,
            bindings::BLE_ATT_ERR_REQ_NOT_SUPPORTED => AttReqNotSupported,
            bindings::BLE_ATT_ERR_INVALID_OFFSET => AttInvalidOffset,
            bindings::BLE_ATT_ERR_INSUFFICIENT_AUTHOR => AttInsufficientAuthor,
            bindings::BLE_ATT_ERR_PREPARE_QUEUE_FULL => AttPrepareQueueFull,
            bindings::BLE_ATT_ERR_ATTR_NOT_FOUND => AttAttrNotFound,
            bindings::BLE_ATT_ERR_ATTR_NOT_LONG => AttAttrNotLong,
            bindings::BLE_ATT_ERR_INSUFFICIENT_KEY_SZ => AttInsufficientKeySize,
            bindings::BLE_ATT_ERR_INVALID_ATTR_VALUE_LEN => AttInvalidAttrValueLen,
            bindings::BLE_ATT_ERR_UNLIKELY => AttUnlikely,
            bindings::BLE_ATT_ERR_INSUFFICIENT_ENC => AttInsufficientEnc,
            bindings::BLE_ATT_ERR_UNSUPPORTED_GROUP => AttUnsupportedGroup,
            bindings::BLE_ATT_ERR_INSUFFICIENT_RES => AttInsufficientRes,
            _ => Other(rc),
        }
    } else {
        let rc_ = rc - bindings::BLE_HS_ERR_HCI_BASE;
        match rc_ {
            bindings::ble_error_codes_BLE_ERR_REM_USER_CONN_TERM => HciConnTermRemote,
            bindings::ble_error_codes_BLE_ERR_UNKNOWN_HCI_CMD => HciUnknownCmd,
            bindings::ble_error_codes_BLE_ERR_UNK_CONN_ID => HciUnknownConnId,
            bindings::ble_error_codes_BLE_ERR_CONN_TERM_LOCAL => HciConnTermLocal,
            bindings::ble_error_codes_BLE_ERR_CONN_ESTABLISHMENT => HciConnEstablishment,
            _ => Other(rc),
        }
    };
    Err(err)
}
