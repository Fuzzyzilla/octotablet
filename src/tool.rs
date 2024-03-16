//! # Tools
//!
//! Also called a *stylus* or *pointer*, these represent the device that the user holds to interact with a pad.
//! "Tools" do not correspond with directly with the above concepts, for example a
//! single stylus with a tip and eraser represents *two* tools, one for each end. These can be re-associated with
//! each other through [`Tool::id`].
//!
//! See [`Type`] for more ideas of what a "tool" may represent.
//!
//! Tools can report zero or more [Axes](crate::axis::Axis) - numerical values describing the characteristics of the user's interaction -
//! which can be used to add greater expression to interactions than standard X,Y motion.
//!
//! In some hardware, and most user configurations a tool is deeply associated with a specific tablet. However,
//! this assumption is not made here, as several hardware vendors allow tools to be used across several connected
//! tablets at once. For current tablet association, listen for the [`In` event](crate::events::ToolEvent::In), being
//! aware that it may change over time.

use crate::axis;
use std::fmt::Debug;

/// An Opaque representation of a Tool button. While tool buttons have no inherent ordering, name, or index, this
/// allows individual buttons on a single tool to be referred to. Though this type is [`Ord`], the ordering provided is
/// arbitrary but stable.
///
/// A single button ID is not necessarily unique across tools.
#[derive(Copy, Clone, Hash, PartialEq, Eq, PartialOrd, Ord)]
#[repr(transparent)]
pub struct ButtonID(pub(crate) crate::platform::ButtonID);
impl std::fmt::Debug for ButtonID {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

/// An opaque identifier that is baked into the hardware of the tool.
/// Likely to remain stable over executions when the same tool hardware is used, and unique across even devices of the same model.
/// It is usable to save per-tool configurations to disk, for example.
///
/// Connects related tools together - for example, a pen and its eraser will share the same hardware id.
#[derive(Copy, Clone, Debug, Hash, PartialEq, Eq, PartialOrd, Ord)]
#[repr(transparent)]
pub struct HardwareID(pub(crate) u64);

#[derive(Clone, Copy, Debug, strum::AsRefStr, PartialEq, Eq)]
pub enum Type {
    Pen,
    Pencil,
    Brush,
    /// A nib found on the reverse of some styli primarily intended to erase.
    /// To associate an eraser with its counterpart, if any, see [`Tool::id`].
    Eraser,
    /// A tool designed to work in the space above the surface, making extensive use of the `Distance` and `Tilt` axes.
    Airbrush,
    /// A touch. Depending on hardware, touches may have additional expressive axes
    /// over a regular touchscreen.
    Finger,
    /// A mouse-like device that rests on the surface and provides absolute coordinates.
    Mouse,
    /// A mouse-like device that rests on the surface with a transparent crosshair for precise selection.
    Lens,
    /// A virtual tool emulated from conventional mouse or touch input. See [`crate::builder::Builder::emulate_tool_from_mouse`].
    Emulated,
}

/// Description of the capabilities of a tool.
#[derive(Debug)]
pub struct Tool {
    /// Platform internal ID.
    pub(crate) internal_id: crate::InternalID,
    pub name: Option<String>,
    /// Identifier uniquely represinting this tool hardware. See [`HardwareID`] for more info.
    ///
    /// If this is present, it's also a hint that the tool may be allowed to freely roam between
    /// several connected tablets. Otherwise, the pen is considered tied to the first tablet it comes
    /// [in proximity](crate::events::ToolEvent::In) to.
    ///
    /// `None` is unknown and does not imply relationships with other tools of hardware id `None`.
    pub hardware_id: Option<HardwareID>,
    /// A unique tool type reported by wacom devices. With a lookup table of wacom hardware,
    /// it is possible to find the specific model and additional capabilities of the device
    /// beyond the properties reported by this crate.
    pub wacom_id: Option<u64>,
    /// Type of the tool, if known.
    pub tool_type: Option<Type>,
    /// The capabilities of the axes reported by this device.
    pub axes: axis::FullInfo,
}

crate::util::macro_bits::impl_get_id!(ID for Tool);
