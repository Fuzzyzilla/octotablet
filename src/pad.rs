//! # Pads
//!
//! Pads represent collections of additional controls that a tablet provides on its surface, including
//! buttons, mode toggles, sliders, wheels, etc. There are typically zero or one pads per [tablet](crate::tablet).
//!
//! A single pad may be further divided into "groups" if several physical clusters of interactables are
//! present - for example, on the *Wacom Cintiq 22HD* a left and right group may be reported. These groups may have
//! independent mode toggles. There are always one or more groups per pad.
//!
//! The number of pads attached to a tablet may be dynamic on particularly esoteric hardware!
//!
//! ## Quirks
//! Not every tablet with buttons or other extra features reports pads. Some *Gaomon* and *XPPEN* tablets, for example,
//! merely emulate keypresses in the driver in response to button clicks, which is transparent to the client and thus not
//! reported by this crate.

use wayland_backend::client::ObjectId;

/// After the user switches to the given mode index, provide feedback to the OS as to the roles
/// of buttons, sliders, and rings. Called potentially many times per switch.
pub type FeedbackFn = dyn FnMut(&Pad, u32, ElementType) -> String;
pub enum ElementType {
    Button,
    Ring,
    Slider,
}

pub struct Pad {
    pub(crate) obj_id: ObjectId,
    pub button_count: u32,
}
impl std::fmt::Debug for Pad {
    fn fmt(&self, fmt: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut this = fmt.debug_struct("Pad");
        let _ = self.obj_id;
        this.field("button_count", &self.button_count);
        this.finish()
    }
}

pub struct PadGroup {
    pub(crate) obj_id: ObjectId,
    pub mode_count: u32,
    /// Called synchronously for each element after a modeswitch on supporting platforms. Provides new description text
    /// for the roles of each element, which may be shown by on-screen displays or other means.
    pub feedback: Option<Box<FeedbackFn>>,
}
impl std::fmt::Debug for PadGroup {
    fn fmt(&self, fmt: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut this = fmt.debug_struct("PadGroup");
        let _ = self.obj_id;
        this.field("mode_count", &self.mode_count);
        let _ = self.feedback;
        this.finish()
    }
}

/// A continuous circular touch-sensitive area or scrollwheel, reporting absolute position in radians clockwise from "logical north."
pub struct PadRing {
    pub(crate) obj_id: ObjectId,
    /// Granularity of the reported angle, if known. This does not affect the range of values.
    ///
    /// For example, if the ring reports a granularity of `32,768`, there are
    /// `32,768` unique angle values between `0` and `TAU` radians.
    pub granularity: Option<u32>,
}
impl std::fmt::Debug for PadRing {
    fn fmt(&self, fmt: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut this = fmt.debug_struct("PadRing");
        let _ = self.obj_id;
        this.field("granularity", &self.granularity);
        this.finish()
    }
}

/// A touch-sensitive strip or slider, reporting absolute position in `0..=1` where 0 is "logical top/left."
pub struct PadStrip {
    pub(crate) obj_id: ObjectId,
    /// Granularity of the reported linear position, if known. This does not affect the range of values.
    ///
    /// For example, if the ring reports a granularity of `32,768`, there are
    /// `32,768` unique slider position values between `0` and `1`.
    pub granularity: Option<u32>,
}
impl std::fmt::Debug for PadStrip {
    fn fmt(&self, fmt: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut this = fmt.debug_struct("PadStrip");
        let _ = self.obj_id;
        this.field("granularity", &self.granularity);
        this.finish()
    }
}
