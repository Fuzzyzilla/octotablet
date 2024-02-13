//! # Tablets
//!
//! A tablet is the logical device providing the system with access to pad and stylus data,
//! and the surface the tools report their interactions with.
//!
//! A tablet provides only limited information about the physical connection to the device -
//! builtin buttons and other tablet hardware are reported by zero or more [pads](crate::pad),
//! and sensing capabilities are provided by individual [tools](crate::tool).

#[derive(Hash, PartialEq, Eq, Debug)]
/// An opaque representation of a tablet, stable and unique as long as this tablet connection
/// exists but not to be considered stable across connections. That is, the same tablet
/// may have differing IDs on different executions, or even after being unplugged and re-plugged.
pub struct ID(crate::InternalID);

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
impl Tablet {
    #[must_use]
    pub fn id(&self) -> ID {
        ID(self.internal_id.clone())
    }
}
