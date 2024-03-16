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
//! emulate keypresses in the driver in response to button clicks, which is transparent to the client and thus not able to be
//! reported by this crate.

pub use group::Group;
pub use ring::Ring;
pub use strip::Strip;

#[derive(Debug)]
pub struct Pad {
    pub(crate) internal_id: crate::InternalID,
    /// How many buttons total are on this pad? Buttons may be further reserved by groups, see [`Group::buttons`] for associating
    /// a button with it's group.
    pub total_buttons: u32,
    /// Groups within this pad. Always at least one.
    // (todo: make that a type-level guarantee)
    pub groups: Vec<Group>,
}
crate::util::macro_bits::impl_get_id!(ID for Pad);
// Submodules for nicer ID names.
pub mod group {
    /// The type of interactable being queried in a [`FeedbackFn`]
    #[derive(Copy, Clone)]
    pub enum FeedbackElement<'a> {
        /// The [button](super::Pad::total_buttons) with the given pad index. This will only
        /// be called with buttons [owned](Group::buttons) by the relevant group.
        Button(u32),
        Ring(&'a super::Ring),
        Strip(&'a super::Strip),
    }

    /// After the group switches to the given mode index, provide feedback to the OS as to the roles
    /// of buttons, sliders, and rings within the group. Called potentially many times per switch.
    pub type FeedbackFn = dyn FnMut(&Group, u32, FeedbackElement<'_>) -> String;

    pub struct Group {
        pub(crate) internal_id: crate::InternalID,
        /// How many mode layers does this group cycle through?
        /// If None, the group does not expose the ability to shift through modes.
        pub mode_count: Option<std::num::NonZeroU32>,
        /// Sorted list of the pad button indices that are owned by this group.
        /// This is some subset of the [buttons reported by the Pad](super::Pad::total_buttons).
        pub buttons: Vec<u32>,
        pub rings: Vec<super::Ring>,
        pub strips: Vec<super::Strip>,
        /// Called synchronously for each group element (buttons, rings, and strips) after a modeswitch on supporting platforms.
        /// Provides new description text for the roles of each element, which may be shown by on-screen displays or other means.
        ///
        /// *This is not guaranteed to be called at any point!*
        pub feedback: Option<Box<FeedbackFn>>,
    }
    // Manual impl since `feedback` is !Debug
    impl std::fmt::Debug for Group {
        fn fmt(&self, fmt: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            let mut this = fmt.debug_struct("PadGroup");
            this.field("internal_id", &self.internal_id);
            this.field("mode_count", &self.mode_count);
            this.field("buttons", &self.buttons);
            this.field("rings", &self.rings);
            this.field("strips", &self.strips);
            // !Debug, so just opaquely show whether it's some or None
            this.field("feeback", &self.feedback.as_ref().map(|_| "..."));
            this.finish()
        }
    }
    crate::util::macro_bits::impl_get_id!(ID for Group);
}

/// The cause of a ring or strip interaction.
#[derive(Eq, PartialEq, Copy, Clone, Debug, Hash)]
pub enum TouchSource {
    Unknown,
    /// A finger is touching the surface.
    Finger,
}
pub mod ring {
    /// A continuous circular touch-sensitive area or scrollwheel, reporting absolute position in radians clockwise from "logical north."
    #[derive(Debug)]
    pub struct Ring {
        pub(crate) internal_id: crate::InternalID,
        /// Granularity of the reported angle, if known.
        pub granularity: Option<crate::axis::Granularity>,
    }
    crate::util::macro_bits::impl_get_id!(ID for Ring);
}
pub mod strip {
    /// A touch-sensitive strip or slider, reporting absolute position in `0..=1` where 0 is "logical top/left."
    #[derive(Debug)]
    pub struct Strip {
        pub(crate) internal_id: crate::InternalID,
        /// Granularity of the reported linear position, if known.
        pub granularity: Option<crate::axis::Granularity>,
    }
    crate::util::macro_bits::impl_get_id!(ID for Strip);
}
