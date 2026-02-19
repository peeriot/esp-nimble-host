use super::bindings;

#[derive(Debug)]
pub enum BleGattAccessContext {
    ReadChar {
        om: *mut bindings::os_mbuf,
        chr: *const bindings::ble_gatt_chr_def,
    },
    WriteChar {
        om: *mut bindings::os_mbuf,
        chr: *const bindings::ble_gatt_chr_def,
    },
    ReadDsc {
        om: *mut bindings::os_mbuf,
        dsc: *const bindings::ble_gatt_dsc_def,
    },
    WriteDsc {
        om: *mut bindings::os_mbuf,
        dsc: *const bindings::ble_gatt_dsc_def,
    },
}

impl From<&bindings::ble_gatt_access_ctxt> for BleGattAccessContext {
    fn from(value: &bindings::ble_gatt_access_ctxt) -> Self {
        match value.op as u32 {
            bindings::BLE_GATT_ACCESS_OP_READ_CHR => {
                let chr = unsafe { value.__bindgen_anon_1.chr };
                BleGattAccessContext::ReadChar { om: value.om, chr }
            }
            bindings::BLE_GATT_ACCESS_OP_WRITE_CHR => {
                let chr = unsafe { value.__bindgen_anon_1.chr };
                BleGattAccessContext::WriteChar { om: value.om, chr }
            }
            bindings::BLE_GATT_ACCESS_OP_READ_DSC => {
                let dsc = unsafe { value.__bindgen_anon_1.dsc };
                BleGattAccessContext::ReadDsc { om: value.om, dsc }
            }
            bindings::BLE_GATT_ACCESS_OP_WRITE_DSC => {
                let dsc = unsafe { value.__bindgen_anon_1.dsc };
                BleGattAccessContext::WriteDsc { om: value.om, dsc }
            }
            _ => panic!("Unknown GATT access context operation: {}", value.op),
        }
    }
}
