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

use std::fmt::Debug;

use wayland_backend::client::ObjectId;

bitflags::bitflags! {
    /// See [`Axis`] for descriptions.
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
    Pressure,
    /// The tool can sense the left-right and forward-back angle from perpendicular.
    Tilt,
    /// The tool can sense a distance from the pad. See [`Tool::distance_unit`] for the interpretation of this axis.
    Distance,
    /// The tool can sense roll angle around it's own axis.
    Roll,
    /// The tool has a scrollwheel. It may report continuous motion as well as discrete steps.
    Wheel,
    /// The tool has a linear slider control, ranging from 0 ("natural" position) to 1.
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
    /// A nib found on the reverse of some styli primarily entended to erase.
    Eraser,
    /// A tool designed to work above the surface of the pad, making extensive
    /// use of the `Distance` and `Tilt` axes.
    Airbrush,
    /// A larger mouse-like device that rests on the pad with a physical transparent crosshair.
    Lens,
    Finger,
    /// An emulated stylus from mouse input.
    Mouse,
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
#[derive(Debug)]
pub struct Tool {
    /// Wayland internal ID.
    pub(crate) obj_id: ObjectId,
    /// An identifier that is baked into the hardware of the tool.
    /// Likely to remain stable over executions, and will also connect related
    /// tools together - for example, a pen and its eraser will share the same id.
    ///
    /// `None` is unknown and does not imply relationships with other tools of id `None`.
    pub id: Option<u64>,
    /// A unique tool type reported by wacom devices. With a lookup table of wacom hardware,
    /// it is possible to find the specific hardware and additional capabilities of the device.
    pub wacom_id: Option<u64>,
    /// Type of the tool, if known.
    pub tool_type: Option<Type>,
    pub available_axes: AvailableAxes,
    pub(crate) axis_info: [AxisInfo; <Axis as strum::EnumCount>::COUNT],
    pub(crate) distance_unit: DistanceUnit,
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
