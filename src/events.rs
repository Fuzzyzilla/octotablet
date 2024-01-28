//! Sequential information about interactions.
//!
//! The events in this module are good for collecting every nuance of the sequence of events
//! and motions that occured since the last frame. For a higher level but lossy
//! view of this information, see the [`summary`] module.
//!
//! In general, if you're making a drawing app, use events to inspect the exact path taken during the frame.
//! If you're using the tools as a mouse-with-extras and only care about the final position, use summaries.

use std::fmt::Debug;
pub mod summary;

use crate::{pad::Pad, tablet::Tablet, tool::Tool, Manager};

/// An opaque, monotonic timestamp with unspecified epoch.
/// The precision of this is given by [`crate::Manager::timestamp_resolution`].
///
/// Subtract two timestamps to get the duration between them, with [`FrameTimestamp::epoch`]
/// being the somewhat-meaningless starting point.
#[derive(Copy, Clone, Hash, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct FrameTimestamp(pub(crate) std::time::Duration);
impl FrameTimestamp {
    /// Get the epoch. This is only useful to subtract from other timestamps.
    ///
    /// This epoch is undefined - the first event could arrive 0s or 3months after the epoch,
    /// it's not to be relied on and is consistent only for comparisons.
    #[must_use]
    pub fn epoch() -> Self {
        Self(std::time::Duration::ZERO)
    }
}
impl std::ops::Sub for FrameTimestamp {
    type Output = std::time::Duration;
    fn sub(self, rhs: Self) -> Self::Output {
        self.0 - rhs.0
    }
}

/// An Option type where NaN is the niche.
// Todo: manual Ord that makes it not partial.
#[derive(Copy, Clone, PartialOrd)]
pub struct NicheF32(f32);
impl NicheF32 {
    /// Wrap a float in this niche, `NaN` coercing to `None`.
    // Not pub cause it might be a footgun lol
    #[must_use]
    const fn wrap(value: f32) -> Self {
        Self(value)
    }
    /// Wrap a non-`NaN` value. Fails with `None` if the value was `NaN`.
    #[must_use]
    pub fn new_some(value: f32) -> Option<Self> {
        (!value.is_nan()).then_some(Self::wrap(value))
    }
    /// Get a `None` niche.
    #[must_use]
    pub const fn new_none() -> Self {
        Self(f32::NAN)
    }
    /// Get the optional value within. If `Some`, guaranteed to not be `NaN`.
    #[must_use]
    pub fn get(self) -> Option<f32> {
        (!self.0.is_nan()).then_some(self.0)
    }
}
impl Default for NicheF32 {
    fn default() -> Self {
        // Not a zero-pattern which is typical of most primitives,
        // but more reasonable than Some(0.0) being the default.
        Self::new_none()
    }
}
impl Debug for NicheF32 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self.get())
    }
}
impl PartialEq for NicheF32 {
    fn eq(&self, other: &Self) -> bool {
        // All NaNs are filtered to None (and considered to be equal here)
        // The remaining f32 comp is Full.
        self.get() == other.get()
    }
}
// One side being non-nan makes it no longer partial!
impl Eq for NicheF32 {}
impl PartialEq<f32> for NicheF32 {
    fn eq(&self, other: &f32) -> bool {
        if let Some(value) = self.get() {
            value == *other
        } else {
            false
        }
    }
}
impl PartialEq<NicheF32> for f32 {
    fn eq(&self, other: &NicheF32) -> bool {
        other == self
    }
}

/// Represents the state of all axes of a tool at some snapshot in time.
///
/// **All axes are Non-`NaN`, finite values.**
///
/// Interpretations of some axes require querying the `Tool` that generated this pose.
///
/// # Quirks
/// There may be axis values that the tool does *not* advertise as available,
/// and axes it advertises may be missing.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Pose {
    /// X, Y position, in pixels from the top left of the associated compositor surface.
    /// This may have subpixel precision, and may exceed your window size in the negative or
    /// positive directions.
    pub position: [f32; 2],
    /// Distance from the surface of the tablet, if available. See the tool's
    /// [`distance_unit`](crate::tool::Tool::distance_unit) for interpretation of the value.
    ///
    /// If `DistanceUnit::Unitless`, this is a normalized `0..=1` value,
    /// otherwise it is unbounded.
    ///
    /// # Quirks
    /// This will not necessarily be zero when in contact with the device, and may
    /// stop updating after contact is reported.
    pub distance: NicheF32,
    /// Perpindicular pressure, if available. `0..=1`
    ///
    /// # Quirks
    /// Full pressure may not correspond to `1.0`.
    pub pressure: NicheF32,
    /// Absolute tilt in randians from perpendicular in the X and Y directions. That is, the first angle
    /// describes the angle between the pen and the Z (perpendicular to the surface) axis along the XZ plane,
    /// and the second angle describes the angle between the pen and Z on the YZ plane.
    ///
    /// `[+,+]` is right+towards user, and `[-,-]` is left+away from user.
    /// # Quirks
    /// In theory the vector `[sin x, sin y]` should describe a projection of the pen's body down on the page,
    /// with length <= 1. However in practice, reported values may break this trigonometric invariant.
    pub tilt: Option<[f32; 2]>,
    /// Absolute roll in radians, if available, around the tool's long axis. `0..<2pi`, where zero is a
    /// hardware-determined "natural" zero-point.
    pub roll: NicheF32,
    /// Absolute scroll wheel angle and clicks in radians, unspecified range or zero-point.
    /// Note that the clicks are *not* a delta.
    pub wheel: Option<(f32, i32)>,
    /// Absolute slider position, if available. `-1..=1`, where zero is the "natural" position.
    pub slider: NicheF32,
}

/// Events associated with a specific `Tool`.
/// Events are logically grouped into "Frames" representing grouping
/// of events in time, providing the timestamp that the group's events
/// occured at if available.
/// Events within a frame are arbitrarily ordered and to be interpreted
/// as having happened similtaneously.
///
/// For example,
/// <pre>
///   /Added
///   |In
///   |Axes
///   \Frame
///   /Axes
///   |Down
///   |Button
///   \Frame
/// </pre>
#[derive(Clone, Copy, Debug)]
pub enum ToolEvent<'a> {
    /// The tool is new. May be enumerated at the start of the program,
    /// or sent immediately before its first use.
    Added,
    /// The tool will no longer send events. It is undefined under what situation this occurs -
    /// it could be on `Out`, when the tablet that discovered it is disconnected, or never.
    /// The tool may immediately be added again before next use, use its [hardware id](crate::tool::Tool::id)
    /// to re-associate it with its past self.
    Removed,
    /// The tool has entered sensing range or entered the window region over the given `tablet`.
    /// Note that this is subject to filtering by the OS -
    /// you may or may not recieve this event when the pen enters sensing range
    /// above a different window.
    In {
        tablet: &'a Tablet,
    },
    /// The tool is considered "pressed." It is implementation defined what the exact semantics are,
    /// but you should treat this as a click or command to start drawing.
    ///
    /// For a pen, this could be contact with the surface with some threshold of force.
    /// For an Airbrush, lens, or mouse, this could be a button.
    Down,
    Button(()),
    /// A snapshot of all axes at this point in time
    // This single variant is so much larger than all the others and inflates the whole
    // event enum by over 2x D:
    Pose(Pose),
    /// The preceding events are submitted as a group, at the given time.
    Frame(Option<FrameTimestamp>),
    /// The tool is no longer pressed.
    Up,
    /// The tool has left sensing range or left the window region of the givent `tablet`.
    Out {
        tablet: &'a Tablet,
    },
}
/// Events associated with a specific `Tablet`.
#[derive(Clone, Copy, Debug)]
pub enum TabletEvent {
    /// The tablet is new. May be enumerated at the start of the program,
    /// may be newly plugged in, or sent immediately before its first use.
    Added,
    /// Unplugged or otherwise becomes unavailable. The tablet will be removed from the hardware report.
    Removed,
}
/// Events associated with a specific `Pad`.
#[derive(Clone, Copy, Debug)]
pub enum PadEvent {
    Added,
    Removed,
    Enter,
    Exit,
    Button(),
    Ring,
    Slider,
}
#[derive(Clone, Copy, Debug)]
pub enum Event<'a> {
    Tool {
        tool: &'a Tool,
        event: ToolEvent<'a>,
    },
    Tablet {
        tablet: &'a Tablet,
        event: TabletEvent,
    },
    Pad {
        pad: &'a Pad,
        event: PadEvent,
    },
}

/// This struct is the primary source of realtime data.
///
/// Opaque, copyable `IntoIterator` over events. Alternatively, if you don't care about
/// intermediate events and just want to know the end state, use [`Events::summarize`].
///
/// Since the returned `Iterator` is unable to rewind, make copies if you need
/// to iterate multiple times. Copies are free!
// We need two objects here because it is *deeply weird* to have a `Copy` Iterator.
#[derive(Clone, Copy)]
pub struct Events<'manager> {
    pub(crate) manager: &'manager Manager,
}
impl<'manager> Events<'manager> {
    /// Get access to the `Manager` that owns these devices and events.
    #[must_use]
    pub fn manager(&'_ self) -> &'manager Manager {
        self.manager
    }
    /// Collect a summary of the final status of connected hardware.
    ///
    /// This is generally much cheaper than iterating, and is useful if you don't
    /// care about intermediate events and just want to know final buttons/positions/etc.
    #[must_use = "returns a new object describing the overall end state"]
    pub fn summarize(self) -> summary::Summary<'manager> {
        self.manager.make_summary()
    }
}
impl<'manager> IntoIterator for Events<'manager> {
    type IntoIter = EventIterator<'manager>;
    type Item = Event<'manager>;
    fn into_iter(self) -> Self::IntoIter {
        EventIterator {
            manager: self.manager,
        }
    }
}
pub struct EventIterator<'manager> {
    pub(crate) manager: &'manager Manager,
}
impl<'manager> EventIterator<'manager> {
    /// Get access to the `Manager` that owns these devices and events.
    #[must_use]
    pub fn manager(&'_ self) -> &'manager Manager {
        self.manager
    }
}
impl<'manager> Iterator for EventIterator<'manager> {
    type Item = Event<'manager>;
    fn next(&mut self) -> Option<Self::Item> {
        None
    }
}
