use alloc::collections::BTreeSet;
use uuid::Uuid;

use crate::characteristic::Characteristic;

/// Represents a Bluetooth service with a unique identifier and a range of handles.
#[derive(Clone, Debug, PartialOrd, Ord, PartialEq, Eq)]
pub struct Service {
    uuid: Uuid,
    start_handle: u16,
    end_handle: u16,
    characteristics: BTreeSet<Characteristic>,
}

impl Service {
    /// Creates a new `Service` with the given UUID, start handle, and end handle.
    pub fn new(uuid: Uuid, start_handle: u16, end_handle: u16) -> Self {
        Self {
            uuid,
            start_handle,
            end_handle,
            characteristics: BTreeSet::new(),
        }
    }

    /// Returns a reference to the set of characteristics.
    pub fn characteristics(&self) -> &BTreeSet<Characteristic> {
        &self.characteristics
    }

    /// Returns a mutable reference to the set of characteristics.
    pub fn characteristics_mut(&mut self) -> &mut BTreeSet<Characteristic> {
        &mut self.characteristics
    }

    /// Returns the start handle of the service.
    pub fn start_handle(&self) -> u16 {
        self.start_handle
    }

    /// Returns the end handle of the service.
    pub fn end_handle(&self) -> u16 {
        self.end_handle
    }

    /// Returns the UUID of the service.
    pub fn uuid(&self) -> Uuid {
        self.uuid
    }
}
