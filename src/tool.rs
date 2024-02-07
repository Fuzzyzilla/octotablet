//! # Tools
//!
//! Also called a *stylus* or *pointer*, these represent the device that the user holds to interact with a pad.
//! "Tools" do not correspond with directly with the above concepts, for example a
//! single stylus with a tip and eraser represents *two* tools, one for each end. These can be re-associated with
//! each other through [`Tool::id`].
//!
//! See [`Type`] for more ideas of what a "tool" may represent.
//!
//! Tools can report zero or more [Axes](Axis) - numerical values describing the characteristics of the user's interaction -
//! which can be used to add greater expression to interactions.
//!
//! In some hardware, and most user configurations a tool is deeply associated with a specific tablet. However,
//! this assumption is not made here, as several hardware vendors allow tools to be used across several connected
//! tablets at once. For current tablet association, listen for the [`In` event](crate::events::ToolEvent::In), being
//! aware that it may change over time.

use std::fmt::Debug;

use crate::platform::InternalID;

bitflags::bitflags! {
    /// Bitflags describing all supported Axes. See [`Axis`] for descriptions.
    #[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
    pub struct AvailableAxes: u16 {
        const PRESSURE = 1;
        const TILT = 2;
        const DISTANCE = 4;
        const ROLL = 8;
        const WHEEL = 16;
        const SLIDER = 32;
    }
}

impl AvailableAxes {
    pub fn iter_axes(&self) -> impl Iterator<Item = Axis> {
        self.intersection(Self::all())
            .iter()
            .map(|flags| match flags {
                Self::PRESSURE => Axis::Pressure,
                Self::TILT => Axis::Tilt,
                Self::DISTANCE => Axis::Distance,
                Self::ROLL => Axis::Roll,
                Self::WHEEL => Axis::Wheel,
                Self::SLIDER => Axis::Slider,
                // We know this is exhaustive due to intersection(all)
                _ => unreachable!(),
            })
    }
}

#[derive(Clone, Copy, Debug, strum::EnumCount, PartialEq, Eq, strum::AsRefStr)]
pub enum Axis {
    /// The tool can sense how much force is applied, purpendicular to the pad surface.
    ///
    /// This does not correspond with any physical units of force and is often configurable via
    /// the tablet driver to have a non-linear response curve.
    Pressure,
    /// The tool can sense the absolute left-right and forward-back tilt angles from perpendicular.
    Tilt,
    /// The tool can sense a distance from the pad. See [`Tool::distance_unit`] for the interpretation of this axis.
    Distance,
    /// The tool can sense absolute roll angle around its own axis.
    Roll,
    /// The tool has a scroll wheel. It may report continuous motion as well as discrete steps.
    Wheel,
    /// The tool has an absolute linear slider control, ranging from -1 to 1 with zero being the "natural" position.
    Slider,
}
impl From<Axis> for AvailableAxes {
    fn from(value: Axis) -> Self {
        match value {
            Axis::Distance => AvailableAxes::DISTANCE,
            Axis::Roll => AvailableAxes::ROLL,
            Axis::Tilt => AvailableAxes::TILT,
            Axis::Pressure => AvailableAxes::PRESSURE,
            Axis::Wheel => AvailableAxes::WHEEL,
            Axis::Slider => AvailableAxes::SLIDER,
        }
    }
}

#[derive(Clone, Copy, Debug, strum::AsRefStr)]
pub enum Type {
    Pen,
    Pencil,
    Brush,
    /// A nib found on the reverse of some styli primarily intended to erase.
    /// To associate an eraser with its counterpart, if any, see [`Tool::id`].
    Eraser,
    /// A tool designed to work above the surface of the pad, making extensive
    /// use of the `Distance` and `Tilt` axes.
    Airbrush,
    /// A touch. Depending on hardware, touches may have additional expressive axes
    /// over a regular touchscreen.
    Finger,
    /// A mouse-like device that rests on the pad and provides absolute coordinates.
    Mouse,
    /// A mouse-like device that rests on the pad with a transparent crosshair for visibility.
    Lens,
}

#[derive(Clone, Copy, Debug, Default)]
pub enum DistanceUnit {
    /// Distance is in an arbitrary normalized number, which may not even be linear.
    #[default]
    Unitless,
    /// The distance is reported as an absolute distance in centimeters.
    /// The maximum sensed distance in cm is included, if available.
    Cm { max: Option<f32> },
}

#[derive(Clone, Copy, Debug, Default)]
pub struct AxisInfo {
    /// Granularity of the axis, if known. This does not affect the range of values.
    ///
    /// For example, if pressure reports a granularity of `32,768`, there are
    /// `32,768` unique pressure values between 0.0 and 1.0.
    pub precision: Option<u32>,
}

/// Description of the capabilities of a tool.
pub struct Tool {
    /// Platform internal ID.
    pub(crate) obj_id: InternalID,
    /// An identifier that is baked into the hardware of the tool.
    /// Likely to remain stable over executions, and unique across even devices of the same model.
    /// It is usable to save per-tool configurations to disk, for example.
    ///
    /// Connects related tools together - for example, a pen and its eraser will share the same id.
    /// `None` is unknown and does not imply relationships with other tools of id `None`.
    ///
    /// If this is present, it's also a hint that the tool may be allowed to freely roam between
    /// several connected tablets. Otherwise, the pen is considered tied to the first tablet it comes
    /// [in proximity](crate::events::ToolEvent::In) to.
    pub id: Option<u64>,
    /// A unique tool type reported by wacom devices. With a lookup table of wacom hardware,
    /// it is possible to find the specific model and additional capabilities of the device.
    pub wacom_id: Option<u64>,
    /// Type of the tool, if known.
    pub tool_type: Option<Type>,
    /// The axes this tool is advertised by the system to report. In practice, this be different
    /// than the actual reported axes.
    pub available_axes: AvailableAxes,
    /// Information about the X,Y axes, which are always supported.
    pub position_info: AxisInfo,
    pub(crate) axis_info: [AxisInfo; <Axis as strum::EnumCount>::COUNT],
    pub(crate) distance_unit: DistanceUnit,
}
impl Debug for Tool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut this = f.debug_struct("Tool");
        let _ = self.obj_id;
        this.field("id", &self.id);
        this.field("wacom_id", &self.wacom_id);
        this.field("tool_type", &self.tool_type);
        this.field("available_axes", &self.available_axes);
        this.field("position_info", &self.position_info);

        let axes: Vec<_> = self
            .available_axes
            .iter_axes()
            .map(|axis| (axis, self.axis(axis).unwrap()))
            .collect();
        this.field("axis_info", &axes);
        if let Some(du) = self.distance_unit() {
            this.field("distance_unit", &du);
        }

        this.finish()
    }
}
impl Tool {
    #[must_use]
    pub fn axis(&self, axis: Axis) -> Option<AxisInfo> {
        self.available_axes
            .contains(axis.into())
            .then_some(self.axis_info[axis as usize])
    }
    /// Query the units of the `Distance` axis. None if this axis isn't supported.
    #[must_use]
    pub fn distance_unit(&self) -> Option<DistanceUnit> {
        self.available_axes
            .contains(AvailableAxes::DISTANCE)
            .then_some(self.distance_unit)
    }
}
