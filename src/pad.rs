//! # Pads
//!
//! Pads represent collections of additional controls that a tablet provides on it's surface, including
//! buttons, mode toggles, sliders, wheels, etc. There are typically zero or one pads per [tablet](crate::tablet).
//!
//! A single pad may be further divided into "groups" if several physical clusters of interactables are
//! present - for example, on the *Wacom Cintiq 22HD* a left and right group may be reported.
//!
//! The number of pads attached to a tablet may be dynamic.

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
    /// Called for each element after a modeswitch on supporting platforms. Provides new description text
    /// for the roles of each element, which may be shown by on-screen displays or other means.
    pub feedback: Option<Box<FeedbackFn>>,
}
impl std::fmt::Debug for Pad {
    fn fmt(&self, _: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let _ = self.feedback;
        Ok(())
    }
}
