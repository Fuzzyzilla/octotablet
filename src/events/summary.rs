//! High-level but limited view at the most recent status of the hardware.

// I reaaaaalllllyy want this to all be borrowed slices. But it's nested three borrows deep
// and that seems impossible without a dubiously-sound self referential owning-form... weh.

use crate::{
    pad::{Group, Pad, Ring, Strip},
    tablet::Tablet,
    tool::Tool,
};

/// The state of a tool when near a tablet.
#[derive(Clone, Copy, Debug)]
pub struct InState<'a> {
    /// The tool in use.
    pub tool: &'a Tool,
    /// The tablet the tool is active near.
    pub tablet: &'a Tablet,
    /// The final pose of the tool.
    pub pose: super::Pose,
    /// Whether the tool is considered "logically pressed".
    /// For most cases, this means the nib is in contact with the surface.
    pub down: bool,
    /// IDs of tool buttons that are currently held down.
    // Counts are not exposed. For the API to be nice, that'd need like... a borrowed btreemap? ech..
    pub pressed_buttons: &'a [u32],
}

/// The state of a tool, if any are currently within sensing range.
#[derive(Clone, Copy, Debug, Default)]
pub enum ToolState<'a> {
    #[default]
    /// There are no tools present.
    Out,
    /// A tool is active
    In(InState<'a>),
}

/// The state of a pad button.
///
/// These are reported globally for the whole pad, but the group that they are owned by
/// is given as well.
#[derive(Clone, Copy, Debug)]
pub struct PadButtonState<'a> {
    /// The group this button is owned by, if any. Use this to associate this button
    /// with the state of its group (eg to query the current mode).
    pub group: Option<&'a Group>,
    /// True if the button is pressed at the time of the summary's capture.
    pub currently_pressed: bool,
    /// How many times the button was pressed since the last summary.
    ///
    /// Presses are counted on the rising edge (ie, going from unpressed to pressed).
    pub count: usize,
}

/// The state of a [`Pad`]
#[derive(Clone, Debug)]
pub struct PadState<'a> {
    /// The pad being reported.
    pub pad: &'a Pad,
    /// The tablet this pad is currently associated with, if any. This may be dynamic on some hardware.
    pub tablet: Option<&'a Tablet>,
    /// A state report for each group belonging to this pad.
    pub groups: Vec<GroupState<'a>>,
    /// A state report for each physical button on this pad, further subdiveded into groups.
    pub buttons: Vec<PadButtonState<'a>>,
}
/// The state of a pad [`Group`]
///
/// Buttons are reported per-pad instead of per-group, see [`PadState::buttons`].
#[derive(Clone, Debug)]
pub struct GroupState<'a> {
    /// The group being reported.
    pub group: &'a Group,
    /// What mode is the group currently in at the time of the summary capture?
    /// This can be used to re-interpret the meanings of buttons, strips, and rings.
    pub mode: Option<u32>,
    /// A state report for each ring in this group.
    pub rings: Vec<RingState<'a>>,
    /// A state report for each strip in this group.
    pub strips: Vec<StripState<'a>>,
}
/// The state of a [`Ring`]
#[derive(Clone, Copy, Debug)]
pub struct RingState<'a> {
    /// The ring being reported.
    pub ring: &'a Ring,
    /// Current absolute angle of the ring, in radians clockwise where 0 is "logical north."
    /// None if not currently known - may require an interaction to discover for the first time.
    pub angle: Option<f32>,
    /// Some if currently pressed, along with a description of the source of the press.
    pub touched_by: Option<crate::pad::TouchSource>,
    /// The change from last summary during a slide, reporting the change in radians clockwise. This value is signed to represent direction,
    /// and may exceed `+/-TAU` - for example `-2TAU` represents two turns counterclockwise during the summary period.
    ///
    /// Only Some during a continuous sliding motion. That is, if the ring is at rest at 0 and a new click makes it jump to 2,
    /// this will not report that jump and will remain None.
    ///
    /// This is provided so that rings may be used to implement scroll gestures easily, which care not for the absolute position
    /// and instead want to listen for gestures.
    pub delta_radians: Option<f32>,
}
/// The state of a [`Strip`]
#[derive(Clone, Copy, Debug)]
pub struct StripState<'a> {
    /// The strip being reported.
    pub strip: &'a Strip,
    /// Current absolute position of the slider, `0..=1` where 0 is "logical top/left".
    /// None if not currently known - may require an interaction to discover for the first time.
    pub position: Option<f32>,
    /// Some if currently pressed, along with a description of the source of the press.
    pub touched_by: Option<crate::pad::TouchSource>,
    /// The change from last summary during a slide. This value is signed to represent direction,
    /// and may exceed one - for example `-2` represents two full-length swipes left/up since the last summary.
    ///
    /// Only Some during a continuous sliding motion. That is, if the strip is at rest at 0 and a new click makes it jump to 1,
    /// this will not report that jump and will remain None.
    ///
    /// This is provided so that strips may be used to implement scroll gestures easily, which care not for the absolute position
    /// and instead want to listen for gestures.
    pub delta: Option<f32>,
}

/// A high-level summary of events. Useful if you don't care about the exact
/// sequence of operations, and just want to know the resting state at the current moment.
#[derive(Clone, Debug)]
pub struct Summary<'a> {
    /// Get the current status of the most recently used tool and the tablet it is active over.
    ///
    /// The summary only exposes a single tool at a time, which unless you're an octopus should be sufficient for
    /// most uses. If you do need to listen to several tools similtaneously, use the [*Events* system](super).
    pub tool: ToolState<'a>,
    /// A report for each pad, giving final positions of sliders and rings, current modes, and counts of button presses during the frame.
    pub pads: Vec<PadState<'a>>,
}
impl Summary<'_> {
    // Hmmst
    #[cfg_attr(not(ink_rts), allow(dead_code))]
    pub(crate) fn empty() -> Self {
        Self {
            tool: ToolState::Out,
            pads: Vec::new(),
        }
    }
}
