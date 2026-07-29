use crate::Drivers;

mod fx2lafw;

/// All built-in drivers.
pub const DRIVERS: Drivers<'static> = Drivers(&[
    fx2lafw::DRIVER,
]);
