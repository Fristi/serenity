//! Async access to the I2C0 bus shared by the AXP2101, touch controller and codec.
//!
//! `embedded-hal-bus` has no async counterpart to its `RefCellDevice`, so this
//! is a minimal one: the bus is borrowed for the duration of a single
//! transaction. Every user runs in the same task, so two borrows can never
//! overlap.

use core::cell::RefCell;

use embedded_hal_async::i2c::{ErrorType, I2c, Operation};

/// Async I2C device on a [`RefCell`]-shared bus.
pub struct AsyncRefCellDevice<'a, T> {
    bus: &'a RefCell<T>,
}

impl<'a, T> AsyncRefCellDevice<'a, T> {
    pub fn new(bus: &'a RefCell<T>) -> Self {
        Self { bus }
    }
}

impl<T: ErrorType> ErrorType for AsyncRefCellDevice<'_, T> {
    type Error = T::Error;
}

impl<T: I2c> I2c for AsyncRefCellDevice<'_, T> {
    async fn transaction(
        &mut self,
        address: u8,
        operations: &mut [Operation<'_>],
    ) -> Result<(), Self::Error> {
        self.bus.borrow_mut().transaction(address, operations).await
    }
}
