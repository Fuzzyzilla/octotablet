//! Sequential information about interactions.
//!
//! The events in this module are good for collecting every nuance of the sequence of events
//! and motions that occured since the last frame. For a higher level but lossy
//! view of this information, see the [`summary`] module.
//!
//! In general, if you're making a drawing app, use events to inspect the exact path taken during the frame.
//! If you're using the tools as a mouse-with-extras and only care about the final position, use summaries.

use std::fmt::Debug;
pub(crate) mod raw;
pub mod summary;

use crate::{pad, platform::PlatformImpl, tablet::Tablet, tool::Tool, Manager};

/// An opaque, monotonic timestamp with unspecified epoch.
/// The precision of this is given by [`crate::Manager::timestamp_granularity`].
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

#[derive(thiserror::Error, Debug)]
pub enum NicheF32Error {
    /// Attempted to make a non-NaN value our of NaN.
    #[error("provided value was NaN")]
    NaN,
}

/// An Option type where NaN is the niche.
// Todo: manual Ord that makes it not partial.
#[derive(Copy, Clone, PartialOrd)]
pub struct NicheF32(f32);
impl NicheF32 {
    pub const NONE: NicheF32 = NicheF32(f32::NAN);
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
        Self::NONE
    }
    /// Get the optional value within. If `Some`, guaranteed to not be `NaN`.
    #[must_use]
    pub fn get(self) -> Option<f32> {
        (!self.0.is_nan()).then_some(self.0)
    }
    /// Create from an [`Option`]. `Some` and `None` variants will correspond exactly with return value of `self.get()`.
    /// # Safety
    /// The value must not be `Some(NaN)`.
    #[must_use]
    pub unsafe fn from_option_unchecked(value: Option<f32>) -> Self {
        unsafe { value.try_into().unwrap_unchecked() }
    }
}
impl TryFrom<Option<f32>> for NicheF32 {
    type Error = NicheF32Error;
    fn try_from(value: Option<f32>) -> Result<Self, Self::Error> {
        if value.is_some_and(f32::is_nan) {
            Err(NicheF32Error::NaN)
        } else {
            // Not Some(NAN), so we can convert.
            Ok(NicheF32(value.unwrap_or(f32::NAN)))
        }
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
/// Interpretations of some axes require querying the `Tool` that generated this pose.
///
/// # Quirks
/// There may be axis values reported that the tool does *not* advertise as available,
/// and axes it does advertise may be missing. These should not necessarily be written off entirely -
/// sometimes it truly has the capability and just fails to advertise it!
// I would *REALLY* like to make the fact that these f32's are non-NaN and finite an invariant, but I literally
// cannot figure out an ergonomic way to do that. Private fields + read-only accessors is one way, but it sucks to use
// for the client. Unsafe wrapper type feels terrible too. Weh!
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
    /// * Pressure is often non-linear, as configured by the user in the driver software.
    /// * Full pressure may not correspond to `1.0`.
    pub pressure: NicheF32,
    /// Absolute tilt in radians from perpendicular in the X and Y directions. That is, the first angle
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
impl Pose {
    pub(crate) fn debug_assert_not_nan(&self) {
        #[cfg(debug_assertions)]
        {
            assert!(!self.position[0].is_nan() && !self.position[1].is_nan());
            if let Some(tilt) = self.tilt {
                assert!(!tilt[0].is_nan() && !tilt[1].is_nan());
            }
            if let Some(wheel) = self.wheel {
                assert!(!wheel.0.is_nan());
            }
        }
    }
}

/// Events associated with a specific `Tool`.
/// Events other than Added and Removed are logically grouped into "Frames" representing grouping
/// of events in time, providing the timestamp that the group's events occured at if available.
/// Events within a frame are arbitrarily ordered and to be interpreted
/// as having happened similtaneously.
///
/// For example,
/// <pre>
/// -Added
/// /In
/// |Pose
/// \Frame
/// /Pose
/// |Button
/// \Frame
/// /Pose
/// |Out
/// \Frame
/// -Removed
/// </pre>
#[derive(Clone, Copy, Debug)]
pub enum ToolEvent<'a> {
    /// The tool is new. May be enumerated at the start of the program,
    /// or sent immediately before its first use. This is not part of a `Frame`.
    Added,
    /// The tool will no longer send events. It is undefined under what situation this occurs -
    /// it could be on `Out`, when the tablet that discovered it is disconnected, or never.
    /// The tool may immediately be added again before next use, use its [hardware id](crate::tool::Tool::hardware_id)
    /// to re-associate it with its past self.
    ///
    /// This is not part of a `Frame`.
    Removed,
    /// The tool has entered sensing range or entered the window region over the given `tablet`.
    /// Note that this is subject to filtering by the OS -
    /// you may or may not recieve this event when the pen enters sensing range
    /// above a different window.
    In { tablet: &'a Tablet },
    /// The tool is considered "pressed." It is implementation defined what the exact semantics are,
    /// but you should treat this as a click or command to start drawing.
    ///
    /// For a pen, this could be contact with the surface with some threshold of force.
    /// For an Airbrush, lens, or mouse, this could be a button.
    Down,
    /// A button on the tool was pressed or released. *This is not an index!*
    Button { button_id: u32, pressed: bool },
    /// A snapshot of all axes at this point in time. See [`Pose`] for quirks.
    // This single variant is so much larger than all the others and inflates the whole
    // event enum by over 2x D:
    Pose(Pose),
    /// The preceding events are submitted as a group, at the given time.
    Frame(Option<FrameTimestamp>),
    /// The tool is no longer pressed.
    Up,
    /// The tool has left sensing range or left the window region of the tablet.
    Out,
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
pub enum PadEvent<'a> {
    /// The pad is new. May be enumerated at the start of the program,
    /// may be newly plugged in, or sent immediately before its first use.
    Added,
    /// Unplugged or otherwise becomes unavailable. The pad will be removed from the hardware report.
    Removed,
    /// Group-specific events
    Group {
        group: &'a pad::Group,
        event: PadGroupEvent<'a>,
    },
    /// A pad button was pressed or released. The button is a zero-based index.
    Button {
        button_idx: u32,
        pressed: bool,
        /// The group that claims ownership of the button, if any.
        group: Option<&'a pad::Group>,
    },
    /// The pad has become associated with the given tablet.
    ///
    /// On some hardware, the physical association with a tablet is dynamic, such as the *Wacom ExpressKey Remote*.
    /// On others, this could be a more permanent association due to it being, well, physically affixed to this tablet!
    Enter { tablet: &'a Tablet },
    /// The pad has lost it's tablet association.
    Exit,
}
/// Events associated with a specific group within a larger Pad.
#[derive(Clone, Copy, Debug)]
pub enum PadGroupEvent<'a> {
    /// A ring was interacted.
    Ring {
        ring: &'a pad::Ring,
        /// Contains the absolute angle, when changed.
        event: TouchStripEvent,
    },
    /// A strip was interacted.
    Strip {
        strip: &'a pad::Strip,
        /// Contains the absolute position, when changed.
        event: TouchStripEvent,
    },
    /// The mode layer was changed to the given mode, zero-indexed. Modes are to be interpreted on a per-group basis, not per-pad.
    ///
    /// You may want to use this to re-interpret meaning to all members of this group, in order to have
    /// several toggle-able layers of controls with a limited number of physical buttons/strips.
    ///
    /// See [`pad::Group::feedback`] for optionally communicating with the system your new usage intents.
    Mode(u32),
}
/// Events for actions on a touch sensitive linear strip or circular ring.
#[derive(Clone, Copy, Debug)]
pub enum TouchStripEvent {
    /// Single degree-of-freedom pose. Interpretation depends on the context under which this event was fired - if from a ring,
    /// this is in radians clockwise from "logical north". If from a strip, it is 0..1 where 0 is "logical top or left".
    Pose(f32),
    /// Optionally sent with a frame to describe the cause of the events.
    Source(pad::TouchSource),
    /// End of a frame. See [`ToolEvent`] for a description of frames. This timestamp is not necessarily
    /// coordinated with other types of `Frame`.
    Frame(Option<FrameTimestamp>),
    /// The interaction is over. This is not guaranteed to be sent at any point.
    ///
    /// This can be used to separate different interactions on the same strip or ring, which is useful for implementing
    /// flick scrolling for example.
    Up,
}
/// Enum over all possible event sources.
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
        pad: &'a pad::Pad,
        event: PadEvent<'a>,
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
        self.manager.internal.make_summary()
    }
}
impl<'manager> IntoIterator for Events<'manager> {
    type IntoIter = EventIterator<'manager>;
    type Item = Event<'manager>;
    fn into_iter(self) -> Self::IntoIter {
        EventIterator {
            manager: self.manager,
            raw: self.manager.internal.raw_events(),
        }
    }
}
pub struct EventIterator<'a> {
    manager: &'a Manager,
    raw: crate::platform::RawEventsIter<'a>,
}
impl<'manager> EventIterator<'manager> {
    /// Get access to the `Manager` that owns these devices and events.
    #[must_use]
    pub fn manager(&'_ self) -> &'manager Manager {
        self.manager
    }
    // Get the next, or Err to retry.
    #[allow(clippy::too_many_lines)]
    fn try_next(&mut self) -> Result<Option<<Self as Iterator>::Item>, ()> {
        use raw::{
            Event as RawEvent, PadEvent as RawPad, TabletEvent as RawTablet, ToolEvent as RawTool,
        };
        let Some(next) = self.raw.next() else {
            return Ok(None);
        };
        Ok(Some(match next {
            RawEvent::Tool { tool, event } => {
                // A linear scan is gonna be much more efficient than the alternatives
                // for any reasonable number of tools. If you have like.... 30 tools at once, then
                // maybe binary search would eek out a win :P
                let tool = self
                    .manager
                    .tools()
                    .iter()
                    .find(|t| t.internal_id == tool)
                    // Fail out (essentially a `filter` for invalid commands...)
                    .ok_or(())?;
                Event::Tool {
                    tool,
                    event: match event {
                        RawTool::Added => ToolEvent::Added,
                        RawTool::Removed => ToolEvent::Removed,
                        RawTool::In { tablet } => ToolEvent::In {
                            tablet: self
                                .manager
                                .tablets()
                                .iter()
                                .find(|t| t.internal_id == tablet)
                                .unwrap(),
                        },
                        RawTool::Down => ToolEvent::Down,
                        RawTool::Button { button_id, pressed } => {
                            ToolEvent::Button { button_id, pressed }
                        }
                        RawTool::Pose(v) => ToolEvent::Pose(v),
                        RawTool::Frame(v) => ToolEvent::Frame(v),
                        RawTool::Up => ToolEvent::Up,
                        RawTool::Out => ToolEvent::Out,
                    },
                }
            }
            RawEvent::Tablet { tablet, event } => {
                let tablet = self
                    .manager
                    .tablets()
                    .iter()
                    .find(|t| t.internal_id == tablet)
                    // Fail out (essentially a `filter` for invalid commands...)
                    .ok_or(())?;
                Event::Tablet {
                    tablet,
                    event: match event {
                        RawTablet::Added => TabletEvent::Added,
                        RawTablet::Removed => TabletEvent::Removed,
                    },
                }
            }
            RawEvent::Pad { pad, event } => {
                let pad = self
                    .manager
                    .pads()
                    .iter()
                    .find(|t| t.internal_id == pad)
                    // Fail out (essentially a `filter` for invalid commands...)
                    .ok_or(())?;
                Event::Pad {
                    pad,
                    event: match event {
                        RawPad::Added => PadEvent::Added,
                        RawPad::Group { group, event } => {
                            let group = pad
                                .groups
                                .iter()
                                .find(|g| g.internal_id == group)
                                // Fail out (essentially a `filter` for invalid commands...)
                                .ok_or(())?;
                            PadEvent::Group {
                                group,
                                event: match event {
                                    raw::PadGroupEvent::Mode(m) => PadGroupEvent::Mode(m),
                                    raw::PadGroupEvent::Ring { ring, event } => {
                                        let ring = group
                                            .rings
                                            .iter()
                                            .find(|r| r.internal_id == ring)
                                            // Fail out (essentially a `filter` for invalid commands...)
                                            .ok_or(())?;
                                        PadGroupEvent::Ring { ring, event }
                                    }
                                    raw::PadGroupEvent::Strip { strip, event } => {
                                        let strip = group
                                            .strips
                                            .iter()
                                            .find(|s| s.internal_id == strip)
                                            // Fail out (essentially a `filter` for invalid commands...)
                                            .ok_or(())?;
                                        PadGroupEvent::Strip { strip, event }
                                    }
                                },
                            }
                        }
                        RawPad::Removed => PadEvent::Removed,
                        RawPad::Button {
                            button_idx,
                            pressed,
                        } => {
                            // Find the group that owns this button, if any.
                            // Not all buttons must be associated with a group!
                            // Unsure of what hardware fits this description, if any...
                            let group = pad.groups.iter().find(|group| {
                                // Sorted, so we can use binary search owo
                                // (tests show that binary search is somehow still more efficient than
                                // linear scan even on trivially smol arrays hehe. this is pointless but fun)
                                group.buttons.binary_search(&button_idx).is_ok()
                            });
                            PadEvent::Button {
                                button_idx,
                                pressed,
                                group,
                            }
                        }
                        RawPad::Enter { tablet } => {
                            let tablet = self
                                .manager
                                .tablets()
                                .iter()
                                .find(|t| t.internal_id == tablet)
                                // Fail out (essentially a `filter` for invalid commands...)
                                .ok_or(())?;
                            PadEvent::Enter { tablet }
                        }
                        RawPad::Exit => PadEvent::Exit,
                    },
                }
            }
        }))
    }
}
impl<'manager> Iterator for EventIterator<'manager> {
    type Item = Event<'manager>;
    fn next(&mut self) -> Option<Self::Item> {
        let mut maybe_next = self.try_next();
        // Infinite loop safety: the inner iter is a slice iter and thus
        // finite in size. try_next always advances.
        while maybe_next.is_err() {
            // report impl bug.
            #[cfg(debug_assertions)]
            {
                eprintln!("[octotablet] implementation bug! failed to build event, skipping");
            }
            maybe_next = self.try_next();
        }
        // While condition says it's Ok
        // this will be Ok(None) on finish.
        maybe_next.unwrap()
    }
}
