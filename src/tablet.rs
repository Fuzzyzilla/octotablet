//! # Tablets
//!
//! A tablet is the logical device providing the system with access to pad and stylus data,
//! and the surface the tools report their interactions with.
//!
//! A tablet provides only limited information about the physical connection to the device -
//! builtin buttons and other tablet hardware are reported by zero or more [pads](crate::pad),
//! and sensing capabilities are provided by individual [tools](crate::tool).

crate::macro_bits::impl_get_id!(ID for Tablet);

#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
pub struct UsbId {
    /// Vendor ID
    pub vid: u16,
    /// Product ID
    pub pid: u16,
}

/// See [module level docs](`crate::tablet`) for details.
#[derive(Debug)]
pub struct Tablet {
    pub(crate) internal_id: crate::InternalID,
    pub name: String,
    pub usb_id: Option<UsbId>,
}
