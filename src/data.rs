use alloc::{ffi::CString, format, string::ToString};
use core::fmt::Debug;

use uuid::Uuid;

use crate::{
    error::DataError,
    nimble_sys::{bindings, return_code_to_result},
};

/// BLE connection handle type.
pub type ConnectionHandle = u16;
/// BLE attribute handle type.
pub type AttributeHandle = u16;

/// BLE address (MAC address and type).
// #[derive(Clone, Serialize, Hash, Deserialize, Eq)]
#[derive(Clone, Hash, PartialEq, Eq)]
pub struct BleAddr {
    pub type_: u8,
    pub addr: [u8; 6],
}

impl Debug for BleAddr {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "BleAddr({})", self)
    }
}

impl BleAddr {
    /// Creates a new BLE address from type and address bytes.
    ///
    /// # Arguments
    ///
    /// * `type_` - Address type (public, random, etc.).
    /// * `addr` - 6-byte MAC address.
    pub fn new(type_: u8, addr: [u8; 6]) -> Self {
        Self { type_, addr }
    }

    /// Parses a BLE address from a string (e.g. `"01:23:45:67:89:ab"`) while
    /// explicitly specifying the BLE address type.
    ///
    /// The input string is expected to be in the usual big-endian, colon-separated
    /// form. Internally, the returned [`BleAddr`] stores the address in the same
    /// little-endian byte order used by NimBLE/HCI (`val[0]` is the least
    /// significant byte), so it can be forwarded directly to `ble_addr_t`.
    ///
    /// # Arguments
    ///
    /// * `type_` - The BLE address type (e.g. public vs random). This value is
    ///   forwarded as-is to NimBLE (`ble_addr_t.type_`).
    /// * `addr` - String representation of the MAC address in the form
    ///   `"01:23:45:67:89:ab"`.
    ///
    /// # Returns
    ///
    /// Returns a [`BleAddr`] on success.
    ///
    /// # Errors
    ///
    /// Returns an error if the string does not contain exactly 6 hex octets or
    /// contains invalid hex characters.
    ///
    /// # Example
    ///
    /// ```ignore
    /// // 0 = public, 1 = random (as used by NimBLE)
    /// let a = BleAddr::parse_str_with_type(1, "cd:7b:13:a5:99:6a")?;
    /// ```
    pub fn parse_str_with_type(type_: u8, addr: &str) -> core::result::Result<Self, DataError> {
        let mut parts: alloc::vec::Vec<u8> = addr
            .split(':')
            .map(|p| u8::from_str_radix(p, 16))
            .collect::<core::result::Result<_, _>>()
            .map_err(|_| {
                DataError::InvalidArgument(format!("Unable to parse MAC address: '{addr}'"))
            })?;

        if parts.len() != 6 {
            return Err(DataError::InvalidArgument(format!(
                "Unable to parse MAC address: '{addr}'"
            )));
        }

        parts.reverse();
        Ok(Self {
            type_,
            addr: parts.try_into().map_err(|_| DataError::BleAddrConversion)?,
        })
    }

    /// Returns the BLE address as a little-endian u64 (lower 6 bytes used).
    ///
    /// # Returns
    ///
    /// The address as a `u64`.
    pub fn as_u64(&self) -> u64 {
        let mut bytes = [0; 8];
        bytes[..6].copy_from_slice(&self.addr);
        u64::from_le_bytes(bytes)
    }
}

impl From<bindings::ble_addr_t> for BleAddr {
    fn from(value: bindings::ble_addr_t) -> Self {
        Self {
            type_: value.type_,
            addr: value.val,
        }
    }
}

impl From<BleAddr> for bindings::ble_addr_t {
    fn from(a: BleAddr) -> Self {
        bindings::ble_addr_t {
            type_: a.type_,
            val: a.addr,
        }
    }
}

impl core::fmt::Display for BleAddr {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let mut addr = self.addr;
        addr.reverse();
        write!(f, "{}", addr.map(|b| format!("{b:02x}")).join(":"))
    }
}

/// BLE GAP discovery parameters wrapper.
#[derive(Clone)]
pub struct BleGapDiscParams(bindings::ble_gap_disc_params);

impl BleGapDiscParams {
    /// Creates new discovery parameters.
    ///
    /// # Arguments
    ///
    /// * `filter_policy` - Filter policy.
    /// * `itvl` - Scan interval.
    /// * `window` - Scan window.
    /// * `limited` - Limited discovery mode.
    /// * `passive` - Passive scanning.
    /// * `filter_duplicates` - Filter duplicate advertisements.
    ///
    /// # Returns
    ///
    /// Returns a new `BleGapDiscParams`.
    pub fn new(
        filter_policy: u8,
        itvl: u16,
        window: u16,
        limited: bool,
        passive: bool,
        filter_duplicates: bool,
    ) -> Self {
        Self(bindings::ble_gap_disc_params {
            filter_policy,
            window,
            itvl,
            _bitfield_align_1: [],
            _bitfield_1: bindings::ble_gap_disc_params::new_bitfield_1(
                limited as _,
                passive as _,
                filter_duplicates as _,
            ),
        })
    }

    /// Gets a reference to the inner C struct.
    ///
    /// # Returns
    ///
    /// Reference to the inner `ble_gap_disc_params`.
    pub fn inner(&self) -> &bindings::ble_gap_disc_params {
        &self.0
    }
}

impl From<bindings::ble_gap_disc_params> for BleGapDiscParams {
    fn from(value: bindings::ble_gap_disc_params) -> Self {
        Self(value)
    }
}

impl From<BleGapDiscParams> for bindings::ble_gap_disc_params {
    fn from(val: BleGapDiscParams) -> Self {
        val.0
    }
}

/// Host advertisement fields parsed from BLE advertisement data.
#[derive(Debug, Clone)]
pub struct HostAdvertismentFields {
    flags: u8,
    uuids16: alloc::vec::Vec<Uuid>,
    uuids32: alloc::vec::Vec<Uuid>,
    uuids128: alloc::vec::Vec<Uuid>,
    name: Option<alloc::string::String>,
    manufacturer_data: alloc::vec::Vec<u8>,
}

impl HostAdvertismentFields {
    /// Gets the advertisement flags.
    ///
    /// # Returns
    ///
    /// The flags byte.
    pub fn flags(&self) -> u8 {
        self.flags
    }

    /// Gets the advertised device name, if present.
    ///
    /// # Returns
    ///
    /// An optional reference to the device name.
    pub fn name(&self) -> Option<&alloc::string::String> {
        self.name.as_ref()
    }

    /// Gets the manufacturer-specific data.
    ///
    /// # Returns
    ///
    /// A byte slice of manufacturer data.
    pub fn manufacturer_data(&self) -> &[u8] {
        self.manufacturer_data.as_ref()
    }

    /// Gets the advertised 16-bit UUIDs.
    ///
    /// # Returns
    ///
    /// A slice of 16-bit UUIDs.
    pub fn uuids16(&self) -> &[Uuid] {
        self.uuids16.as_ref()
    }

    /// Gets the advertised 32-bit UUIDs.
    ///
    /// # Returns
    ///
    /// A slice of 32-bit UUIDs.
    pub fn uuids32(&self) -> &[Uuid] {
        self.uuids32.as_ref()
    }

    /// Gets the advertised 128-bit UUIDs.
    ///
    /// # Returns
    ///
    /// A slice of 128-bit UUIDs.
    pub fn uuids128(&self) -> &[Uuid] {
        self.uuids128.as_ref()
    }
}

impl From<bindings::ble_hs_adv_fields> for HostAdvertismentFields {
    fn from(value: bindings::ble_hs_adv_fields) -> Self {
        let uuids16 = if value.num_uuids16 > 0 {
            let uuids16_slice = unsafe {
                core::slice::from_raw_parts::<bindings::ble_uuid16_t>(
                    value.uuids16,
                    value.num_uuids16 as usize,
                )
            };

            uuids16_slice
                .iter()
                .map(|uuid16| uuid_from_u16(uuid16.value))
                .collect()
        } else {
            alloc::vec::Vec::new()
        };

        let uuids32 = if value.num_uuids32 > 0 {
            let uuids32_slice = unsafe {
                core::slice::from_raw_parts::<bindings::ble_uuid32_t>(
                    value.uuids32,
                    value.num_uuids32 as usize,
                )
            };

            uuids32_slice
                .iter()
                .map(|uuid32| uuid_from_u32(uuid32.value))
                .collect()
        } else {
            alloc::vec::Vec::new()
        };

        let uuids128 = if value.num_uuids128 > 0 {
            let uuids128_slice = unsafe {
                core::slice::from_raw_parts::<bindings::ble_uuid128_t>(
                    value.uuids128,
                    value.num_uuids128 as usize,
                )
            };

            uuids128_slice
                .iter()
                .map(|uuid128| Uuid::from_bytes(uuid128.value))
                .collect()
        } else {
            alloc::vec::Vec::new()
        };

        let name = if value.name_len > 0 {
            let device_name_slice =
                unsafe { core::slice::from_raw_parts::<u8>(value.name, value.name_len as usize) };
            if let Ok(device_name_c_string) = CString::new(device_name_slice) {
                Some(device_name_c_string.to_string_lossy().to_string())
            } else {
                None
            }
        } else {
            None
        };

        let manufacturer_data = if value.mfg_data_len > 0 {
            let mfg_data_slice = unsafe {
                core::slice::from_raw_parts::<u8>(value.mfg_data, value.mfg_data_len as usize)
            };
            mfg_data_slice.to_vec()
        } else {
            alloc::vec::Vec::new()
        };

        Self {
            flags: value.flags,
            uuids16,
            uuids32,
            uuids128,
            name,
            manufacturer_data,
        }
    }
}

#[derive(Clone, Debug)]
pub struct RawAdvertisement {
    addr: BleAddr,
    rssi: i8,
    data: heapless::Vec<u8, 255>,
}

impl RawAdvertisement {
    pub fn new(addr: BleAddr, rssi: i8, data: heapless::Vec<u8, 255>) -> Self {
        Self { addr, rssi, data }
    }

    pub fn addr(&self) -> &BleAddr {
        &self.addr
    }

    pub fn rssi(&self) -> i8 {
        self.rssi
    }

    /// Raw BLE advertisement bytes (AD structure).
    pub fn data(&self) -> &[u8] {
        &self.data
    }
}

/// BLE advertisement, including address, RSSI, and parsed fields.
#[derive(Debug, Clone)]
pub struct Advertisement {
    addr: BleAddr,
    rssi: i8,
    fields: HostAdvertismentFields,
}

impl Advertisement {
    /// Creates a new Advertisement.
    ///
    /// # Arguments
    ///
    /// * `addr` - BLE address of the advertiser.
    /// * `rssi` - Received Signal Strength Indicator.
    /// * `fields` - Parsed advertisement fields.
    ///
    /// # Returns
    ///
    /// Returns a new `Advertisement`.
    pub fn new(addr: BleAddr, rssi: i8, fields: HostAdvertismentFields) -> Self {
        Self { addr, rssi, fields }
    }

    /// Gets the BLE address of the advertiser.
    ///
    /// # Returns
    ///
    /// Reference to the `BleAddr`.
    pub fn addr(&self) -> &BleAddr {
        &self.addr
    }

    /// Gets the parsed advertisement fields.
    ///
    /// # Returns
    ///
    /// Reference to the `HostAdvertismentFields`.
    pub fn fields(&self) -> &HostAdvertismentFields {
        &self.fields
    }

    /// Gets the RSSI value.
    ///
    /// # Returns
    ///
    /// The RSSI as `i8`.
    pub fn rssi(&self) -> i8 {
        self.rssi
    }
}

impl TryFrom<RawAdvertisement> for Advertisement {
    type Error = DataError;

    fn try_from(value: RawAdvertisement) -> Result<Self, Self::Error> {
        let mut fields: bindings::ble_hs_adv_fields = unsafe { core::mem::zeroed() };
        let ret = unsafe {
            bindings::ble_hs_adv_parse_fields(
                &mut fields,
                value.data.as_ptr(),
                value.data.len() as u8,
            )
        };
        return_code_to_result(ret as u32, ()).map_err(DataError::AdvParseFields)?;
        let fields = HostAdvertismentFields::from(fields);
        Ok(Self {
            addr: value.addr,
            rssi: value.rssi,
            fields,
        })
    }
}

const BLUETOOTH_BASE_UUID: u128 = 0x00000000_0000_1000_8000_00805f9b34fb;
const BLUETOOTH_BASE_MASK: u128 = 0x00000000_ffff_ffff_ffff_ffffffffffff;
const BLUETOOTH_BASE_MASK_16: u128 = 0xffff0000_ffff_ffff_ffff_ffffffffffff;

// TODO: Make these functions part of the `BleUuid` trait once const fn is allowed there.

/// Converts a 32-bit BLE short UUID to a full 128-bit UUID by filling in the standard Bluetooth Base UUID.
///
/// # Arguments
///
/// * `short` - The 32-bit short UUID.
///
/// # Returns
///
/// Returns a full 128-bit `Uuid`.
pub const fn uuid_from_u32(short: u32) -> Uuid {
    Uuid::from_u128(BLUETOOTH_BASE_UUID | ((short as u128) << 96))
}

/// Converts a 16-bit BLE short UUID to a full 128-bit UUID by filling in the standard Bluetooth Base UUID.
///
/// # Arguments
///
/// * `short` - The 16-bit short UUID.
///
/// # Returns
///
/// Returns a full 128-bit `Uuid`.
pub const fn uuid_from_u16(short: u16) -> Uuid {
    uuid_from_u32(short as u32)
}

/// Enum representing any supported BLE UUID type.
pub enum NimbleUuid {
    Uuid16(bindings::ble_uuid16_t),
    Uuid32(bindings::ble_uuid32_t),
    Uuid128(bindings::ble_uuid128_t),
}

impl NimbleUuid {
    /// Gets a pointer to the underlying C UUID struct.
    ///
    /// # Returns
    ///
    /// Pointer to the `ble_uuid_t`.
    pub fn raw_ptr(&self) -> *const bindings::ble_uuid_t {
        match self {
            NimbleUuid::Uuid16(uuid) => &uuid.u,
            NimbleUuid::Uuid32(uuid) => &uuid.u,
            NimbleUuid::Uuid128(uuid) => &uuid.u,
        }
    }
}

/// Converts a Rust Uuid to a NimbleUuid (16, 32, or 128 bit).
///
/// # Arguments
///
/// * `uuid` - The Rust `Uuid` to convert.
///
/// # Returns
///
/// Returns a `NimbleUuid` variant matching the UUID width.
pub fn uuid_to_nimble_uuid(uuid: &Uuid) -> NimbleUuid {
    let value = uuid.as_u128();

    if value & BLUETOOTH_BASE_MASK_16 == BLUETOOTH_BASE_UUID {
        let value = ((value & !BLUETOOTH_BASE_MASK_16) >> 96) as u16;
        NimbleUuid::Uuid16(bindings::ble_uuid16_t {
            u: bindings::ble_uuid_t { type_: 16 },
            value,
        })
    } else if value & BLUETOOTH_BASE_MASK == BLUETOOTH_BASE_UUID {
        let value = ((value & !BLUETOOTH_BASE_MASK) >> 96) as u32;
        NimbleUuid::Uuid32(bindings::ble_uuid32_t {
            u: bindings::ble_uuid_t { type_: 32 },
            value,
        })
    } else {
        // uuid::Uuid stores bytes in big-endian (RFC 4122) order;
        // NimBLE expects little-endian BLE wire order.
        let mut value = uuid.into_bytes();
        value.reverse();
        NimbleUuid::Uuid128(bindings::ble_uuid128_t {
            u: bindings::ble_uuid_t { type_: 128 },
            value,
        })
    }
}

/// Converts a Nimble C UUID struct to a Rust Uuid.
///
/// # Arguments
///
/// * `uuid` - Pointer to a Nimble `ble_uuid_any_t`.
///
/// # Returns
///
/// Returns a Rust `Uuid` if conversion is successful, otherwise an error.
pub fn nimble_uuid_to_uuid(
    uuid: &bindings::ble_uuid_any_t,
) -> core::result::Result<Uuid, DataError> {
    unsafe {
        match uuid.u.type_ as _ {
            bindings::BLE_UUID_TYPE_16 => Ok(uuid_from_u16(uuid.u16_.value)),
            bindings::BLE_UUID_TYPE_32 => Ok(uuid_from_u32(uuid.u32_.value)),
            bindings::BLE_UUID_TYPE_128 => {
                // NimBLE stores 128-bit UUIDs in BLE little-endian wire order;
                // uuid::Uuid expects big-endian (RFC 4122) byte order.
                let mut bytes = uuid.u128_.value;
                bytes.reverse();
                match Uuid::from_slice(&bytes) {
                    Ok(uuid) => Ok(uuid),
                    Err(err) => Err(DataError::UuidConversion(format!(
                        "Unable to decode 128bit UUID: {err}"
                    ))),
                }
            }
            _ => Err(DataError::UuidConversion(format!(
                "Invalid UUID type: {}",
                uuid.u.type_
            ))),
        }
    }
}
