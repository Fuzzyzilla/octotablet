//! High-level but limited view at the most recent status of the hardware.

use crate::{pad::Pad, tablet::Tablet, tool::Tool};

#[derive(Clone, Copy, Debug)]
pub struct InState<'a> {
    /// The tool in use
    pub tool: &'a Tool,
    /// The tablet the tool is active near
    pub tablet: &'a Tablet,
    /// The final pose of the tool
    pub pose: super::Pose,
    /// Whether the tool is considered "clicked".
    pub down: bool,
    /// Timestamp of the given pose.
    pub timestamp: Option<super::FrameTimestamp>,
    /// IDs of tool buttons that are currently held down.
    pub pressed_buttons: &'a [u32],
}

#[derive(Clone, Copy, Debug)]
pub enum ToolState<'a> {
    /// There are no tools present.
    Out,
    /// A tool is active
    In(InState<'a>),
}

#[derive(Clone, Copy, Debug)]
pub struct PadState<'a> {
    pub pad: &'a Pad,
}

/// Reports a high-level summary of events. Useful if you don't care about the exact
/// sequence of operations, and just want to know the resting state at the current moment.
#[derive(Clone, Copy, Debug)]
pub struct Summary<'a> {
    /// Get the current status of the most recently used tool and the tablet
    /// it is active over.
    ///
    /// The summary only exposes a single tool at a time, which unless you're an octopus should be sufficient for
    /// most uses - many systems don't adequately support more than one pointing device at a time anyway.
    /// If you do need this, revert to the *Events* system.
    pub tool: ToolState<'a>,
    /// A report for each pad, reporting final positions of sliders and rings, and counts of button presses during the events.
    /// No ordering information is provided between events, i.e. a value of 2 for buttons `a` and `b` could have been
    /// `[a, a, b, b]` or `[b, a, b, a]`, etc. Use the *Events* system if you care about the ordering.
    pub pads: &'a [PadState<'a>],
}
