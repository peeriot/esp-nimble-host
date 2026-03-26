use alloc::vec::Vec;
use uuid::Uuid;

use crate::characteristic::Characteristic;

/// Represents a Bluetooth service with a unique identifier and a range of handles.
#[derive(Clone, Debug)]
pub struct Service {
    uuid: Uuid,
    start_handle: u16,
    end_handle: u16,
    characteristics: Vec<Characteristic>,
}

impl Service {
    /// Creates a new `Service` with the given UUID, start handle, and end handle.
    pub fn new(uuid: Uuid, start_handle: u16, end_handle: u16) -> Self {
        Self {
            uuid,
            start_handle,
            end_handle,
            characteristics: Vec::new(),
        }
    }

    /// Returns a reference to the characteristics.
    pub fn characteristics(&self) -> &[Characteristic] {
        &self.characteristics
    }

    /// Returns a mutable reference to the characteristics.
    pub fn characteristics_mut(&mut self) -> &mut Vec<Characteristic> {
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
