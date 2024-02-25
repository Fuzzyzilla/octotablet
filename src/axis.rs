use crate::util::NicheF32;

bitflags::bitflags! {
    /// Bitflags describing all supported Axes. See [`Axis`] for descriptions.
    ///
    /// X and Y axes are implicit and always available.
    #[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
    pub struct AvailableAxes: u16 {
        // Vaguely in order of common-ness.
        const PRESSURE = 1;
        const TILT = 2;
        const DISTANCE = 4;
        const ROLL = 8;
        const WHEEL = 16;
        const SLIDER = 32;
        const BUTTON_PRESSURE = 64;
        const CONTACT_SIZE = 128;
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
                Self::BUTTON_PRESSURE => Axis::ButtonPressure,
                Self::CONTACT_SIZE => Axis::ContactSize,
                // We know this is exhaustive due to intersection(all)
                // Additions to Self are not syntax errors here, i wish i could make them so!
                _ => unreachable!(),
            })
    }
}

#[derive(
    Clone,
    Copy,
    Debug,
    strum::EnumCount,
    PartialEq,
    Eq,
    strum::AsRefStr,
    strum::IntoStaticStr,
    strum::EnumIter,
)]
pub enum Axis {
    /// The tool can sense how much force is applied, purpendicular to the pad surface.
    Pressure,
    /// The tool can sense the absolute left-right and forward-back tilt angles from perpendicular.
    Tilt,
    /// The tool can sense a distance from the pad.
    Distance,
    /// The tool can sense absolute roll angle around its own axis.
    Roll,
    /// The tool has a scroll wheel. It may report continuous motion as well as discrete steps.
    Wheel,
    /// The tool has an absolute linear slider control, ranging from -1 to 1 with zero being the "natural" position.
    Slider,
    /// The tool has a pressure-sensitive button, reporting how hard the user is pressing on it.
    ButtonPressure,
    /// The tool can sense the XY-axis-aligned size of the surface contact ellipse.
    ContactSize,
    // /// The tool reports how sure it is a given contact is truly a contact.
    // I uh.. don't think this is.. particularly useful lol.
    // If you need it, feel free to submit an issue
    // ContactConfidence,
}
impl From<Axis> for AvailableAxes {
    fn from(value: Axis) -> Self {
        match value {
            Axis::Pressure => AvailableAxes::PRESSURE,
            Axis::Distance => AvailableAxes::DISTANCE,
            Axis::Roll => AvailableAxes::ROLL,
            Axis::Tilt => AvailableAxes::TILT,
            Axis::Wheel => AvailableAxes::WHEEL,
            Axis::Slider => AvailableAxes::SLIDER,
            Axis::ButtonPressure => AvailableAxes::BUTTON_PRESSURE,
            Axis::ContactSize => AvailableAxes::CONTACT_SIZE,
        }
    }
}

pub mod unit {
    //! Descriptions of how values reported by the hardware are to be interpreted.

    #[derive(
        Clone, Copy, Debug, Default, PartialEq, Eq, Hash, strum::AsRefStr, strum::IntoStaticStr,
    )]
    pub enum Linear {
        /// An arbitrary number, which may not even have linear correlation with a physical unit.
        /// Zero point is still guaranteed to match the zero point of a physical unit.
        #[default]
        Unitless,
        /// Absolute value reported in centimeters.
        Centimeters,
    }
    #[derive(
        Clone, Copy, Debug, Default, PartialEq, Eq, Hash, strum::AsRefStr, strum::IntoStaticStr,
    )]
    pub enum Force {
        /// An arbitrary number, which may not even have linear correlation with a physical unit.
        /// Zero point is still guaranteed to match the zero point of a physical unit.
        #[default]
        Unitless,
        /// Absolute value reported in grams.
        Grams,
    }
    #[derive(
        Clone, Copy, Debug, Default, PartialEq, Eq, Hash, strum::AsRefStr, strum::IntoStaticStr,
    )]
    pub enum Angle {
        /// An arbitrary number, which may not even have linear correlation with a physical unit.
        /// Zero point is still guaranteed to match the zero point of a physical unit.
        #[default]
        Unitless,
        /// Absolute value reported in radians.
        Radians,
    }
}

/// Granularity of an axis, if known. This does not affect the range of values.
///
/// For example, if pressure reports a granularity of `32,768`, there are
/// `32,768` unique pressure values between 0.0 and 1.0.
#[derive(Clone, Copy, Debug, Default)]
#[repr(transparent)]
pub struct Granularity(pub u32);

/// Limits of an axis's reported value.
/// # Quirks
/// This is a hint, and the value is not clamped - the hardware is allowed to report a value exceeding this in either direction.
#[derive(Clone, Copy, Debug, Default)]
pub struct Limits {
    pub min: f32,
    pub max: f32,
}

// hm.. this isn't great x3
// being generic over unit type, limited to the three types, would probably be extra trouble, though.
#[derive(Clone, Copy, Debug, Default)]
pub struct LinearInfo {
    pub unit: unit::Linear,
    pub info: Info,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct ForceInfo {
    pub unit: unit::Force,
    pub info: Info,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct AngleInfo {
    pub unit: unit::Angle,
    pub info: Info,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Info {
    pub limits: Option<Limits>,
    pub granularity: Option<Granularity>,
}

/// A report of the limits and capabilities of all axes, or None if the axis is
/// not supported by the device.
// !Copy since it's LARGE AS HECK and thus implicit copying *should* be an error to
// bonk you gently into using a ref instead.
#[derive(Debug, Clone, Default)]
pub struct FullInfo {
    // Fixed unit axes
    /// The X, Y axes are always supported, and with units of logical pixels.
    // This explicitly denies the existance of Centimeter based coordinates, which
    // could be used for apps that want that kind of physical accuracy. Idk how to make
    // both possibilities play nicely, however.
    pub position: Info,
    /// A unitless normalized value in [-1, 1]
    pub slider: Option<Info>,
    /// Always radians.
    pub roll: Option<Info>,
    // Force axes
    pub pressure: Option<ForceInfo>,
    pub button_pressure: Option<ForceInfo>,
    // Angular axes
    pub tilt: Option<AngleInfo>,
    pub wheel: Option<AngleInfo>,
    // Linear axes
    pub distance: Option<LinearInfo>,
    pub contact_size: Option<LinearInfo>,
}

#[derive(thiserror::Error, Debug, Copy, Clone, PartialEq, Eq, Hash)]
#[error("axis not supported")]
pub struct UnsupportedAxisError;

impl FullInfo {
    /// Get a bitmask summary of the available axes.
    #[rustfmt::skip]
    #[must_use]
    pub fn available(&self) -> AvailableAxes {
        let empty = AvailableAxes::empty();

        self.slider.map_or(empty, |_| AvailableAxes::SLIDER)
        | self.roll.map_or(empty, |_| AvailableAxes::ROLL)
        | self.pressure.map_or(empty, |_| AvailableAxes::PRESSURE)
        | self.button_pressure.map_or(empty, |_| AvailableAxes::BUTTON_PRESSURE)
        | self.tilt.map_or(empty, |_| AvailableAxes::TILT)
        | self.wheel.map_or(empty, |_| AvailableAxes::WHEEL)
        | self.distance.map_or(empty, |_| AvailableAxes::DISTANCE)
        | self.contact_size.map_or(empty, |_| AvailableAxes::CONTACT_SIZE)
    }
    #[allow(clippy::missing_errors_doc)]
    pub fn granularity(&self, axis: Axis) -> Result<Option<Granularity>, UnsupportedAxisError> {
        self.info(axis).map(|info| info.granularity)
    }
    #[allow(clippy::missing_errors_doc)]
    pub fn limits(&self, axis: Axis) -> Result<Option<Limits>, UnsupportedAxisError> {
        self.info(axis).map(|info| info.limits)
    }
    #[allow(clippy::missing_errors_doc)]
    pub fn info(&self, axis: Axis) -> Result<&Info, UnsupportedAxisError> {
        match axis {
            Axis::Pressure => self
                .pressure
                .as_ref()
                .map(|a| &a.info)
                .ok_or(UnsupportedAxisError),
            Axis::Tilt => self
                .tilt
                .as_ref()
                .map(|a| &a.info)
                .ok_or(UnsupportedAxisError),
            Axis::Distance => self
                .distance
                .as_ref()
                .map(|a| &a.info)
                .ok_or(UnsupportedAxisError),
            Axis::Roll => self.roll.as_ref().ok_or(UnsupportedAxisError),
            Axis::Wheel => self
                .wheel
                .as_ref()
                .map(|a| &a.info)
                .ok_or(UnsupportedAxisError),
            Axis::Slider => self.slider.as_ref().ok_or(UnsupportedAxisError),
            Axis::ButtonPressure => self
                .button_pressure
                .as_ref()
                .map(|a| &a.info)
                .ok_or(UnsupportedAxisError),
            Axis::ContactSize => self
                .contact_size
                .as_ref()
                .map(|a| &a.info)
                .ok_or(UnsupportedAxisError),
        }
    }
}

/// Represents the state of all axes of a tool at some snapshot in time.
///
/// Interpretations, units, and minimas/maximas of some axes require querying the `Tool` that generated this pose's [`FullInfo`].
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
    /// X, Y position, in logical pixels (ie, already divided by the scale factor) from the top left of the
    /// associated window. This may have sub-pixel precision, and may exceed your window size in the negative or
    /// positive directions.
    pub position: [f32; 2],
    /// Distance from the surface of the tablet.
    ///
    /// # Quirks
    /// This will not necessarily be zero when in contact with the device, and may
    /// stop updating after contact is reported.
    pub distance: NicheF32,
    /// The force the nib is pressed with.
    ///
    /// # Quirks
    /// * Pressure is often non-linear, as configured by the user in the driver software.
    /// * Full pressure may not reach the presure axis' [`max`](Limits::max).
    pub pressure: NicheF32,
    /// Absolute tilt from perpendicular in the X and Y directions. That is, the first angle
    /// describes the angle between the pen and the Z (perpendicular to the surface) axis along the XZ plane,
    /// and the second angle describes the angle between the pen and Z on the YZ plane.
    ///
    /// `[+,+]` is right+towards user, and `[-,-]` is left+away from user.
    /// # Quirks
    /// In theory the vector `[sin x, sin y]` (assuming [radians](AngleInfo::unit)
    /// are reported) should describe a projection of the pen's body down on the page, with length <= 1.
    /// However in practice, reported values may break this trigonometric invariant.
    pub tilt: Option<[f32; 2]>,
    /// Absolute roll in radians, around the tool's long axis. Zero is a hardware-determined "natural" angle.
    pub roll: NicheF32,
    /// Absolute scroll wheel angle and clicks in radians, unspecified range or zero-point.
    /// Note that the clicks are *not* a delta.
    pub wheel: Option<(f32, i32)>,
    /// Absolute slider position, in `[-1, 1]`, where zero is the "natural" position.
    pub slider: NicheF32,
    /// The force on a pressure-sensitive button.
    pub button_pressure: NicheF32,
    /// The size of the contact ellipse. `0` describes the X-axis width of the ellipse, and `1` describes the Y-axis height.
    pub contact_size: Option<[f32; 2]>,
}
