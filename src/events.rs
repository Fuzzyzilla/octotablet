use std::fmt::Debug;

use crate::{pad::Pad, tablet::Tablet, tool::Tool};

/// An opaque, monotonic timestamp with unspecified epoch.
/// The precision of this is given by [`crate::Manager::timestamp_resolution`].
///
/// Subtract two timestamps to get the duration between them, or use [`FrameTimestamp::since_arbitrary`]
/// for a somewhat meaningless duration since the epoch.
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct FrameTimestamp(std::time::Duration);
impl FrameTimestamp {
    /// Get the time since the unspecified epoch.
    #[must_use]
    pub fn since_arbitrary(self) -> std::time::Duration {
        self.0
    }
}
impl std::ops::Sub for FrameTimestamp {
    type Output = std::time::Duration;
    fn sub(self, rhs: Self) -> Self::Output {
        self.0 - rhs.0
    }
}

struct AxisEventPack();
/// Represents the state of all axes of a tool at some point in time.
#[derive(Clone, Copy)]
pub struct Axes<'a> {
    _pack: &'a AxisEventPack,
    _position: (),
}
impl Debug for Axes<'_> {
    fn fmt(&self, _: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        todo!()
    }
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
///
/// Notably - there is no `Removed` event. There is no concept of a tool being removed,
/// only leaving proximity (`Out`). Tools live for as long as the [`Manager`](crate::Manager) exists.
#[derive(Clone, Copy, Debug)]
pub enum ToolEvent<'a> {
    /// The tool is new. May be enumerated at the start of the program,
    /// or sent immediately before it's first use.
    Added,
    /// The tool has entered sensing range or entered the window region over the given `tablet`.
    /// Note that this is subject to filtering by the OS -
    /// you may or may not recieve this event when the pen enters sensing range
    /// above a different window.
    In {
        tablet: &'a Tablet,
    },
    /// The tool is considered "pressed."
    /// For a pen, this could be contact with the surface. For an Airbrush, lens, or mouse,
    /// this could be a button.
    Down,
    Button(()),
    Axes(Axes<'a>),
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
    /// may be newly plugged in, or sent immediately before it's first use.
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
    Button,
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

pub(crate) struct EventIterator<'manager> {
    pub(crate) _manager: &'manager crate::Manager,
}

impl<'manager> Iterator for EventIterator<'manager> {
    type Item = Event<'manager>;
    fn next(&mut self) -> Option<Self::Item> {
        None
    }
}
