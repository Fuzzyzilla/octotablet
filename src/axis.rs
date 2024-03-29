//! Axes descriptions of tool capabilities and limits.

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
/// An individual Tool axis.
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
    /// The tool has an absolute linear slider control, `[-1, 1]` with zero being the "natural" position.
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

pub(crate) trait Union {
    /// Take the union of two iteems, returning the widest spreading of both.
    #[must_use = "doesn't modify self, returns a new Info representing the union"]
    fn union(&self, other: &Self) -> Self;
}
impl<U: Union + Clone> Union for Option<U> {
    fn union(&self, other: &Self) -> Self {
        match (self, other) {
            (Some(a), Some(b)) => Some(a.union(b)),
            (Some(x), None) | (None, Some(x)) => Some(x.clone()),
            (None, None) => None,
        }
    }
}

/// Describes the number of unique values in the entire range of the associated axis.
///
/// This does not affect the range of values nor the interpretation of values reported by a [`Pose`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[repr(transparent)]
pub struct Granularity(pub std::num::NonZeroU32);

impl Union for Granularity {
    fn union(&self, other: &Self) -> Self {
        Self(self.0.max(other.0))
    }
}

/// Granularity of a position axis. Since the bounds of the range of positions are not known,
/// this instead represents as "dots per logical pixel" contrary to other axes' [`Granularity`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[repr(transparent)]
pub struct PositionGranularity(pub std::num::NonZeroU32);

impl Union for PositionGranularity {
    fn union(&self, other: &Self) -> Self {
        Self(self.0.max(other.0))
    }
}

/// Limits of an axis's reported value. This does not affect the interpretation of a value reported by [`Pose`]
///
/// In many cases, you may ignore this value - this crate makes strong guarantees about the
/// units values are reported in, and you may rely on those. However, this may desscribe e.g. that a tool only detects
/// tilt in the +/-45° range.
/// # Quirks
/// This is a hint, and the value is not clamped - the hardware is allowed to report a value exceeding this in either direction.
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    /// Inclusive minimum
    pub min: f32,
    /// Inclusive maximum
    pub max: f32,
}
impl From<Limits> for std::ops::RangeInclusive<f32> {
    fn from(value: Limits) -> std::ops::RangeInclusive<f32> {
        value.min..=value.max
    }
}
impl From<std::ops::RangeInclusive<f32>> for Limits {
    fn from(value: std::ops::RangeInclusive<f32>) -> Self {
        let (min, max) = value.into_inner();
        Self { min, max }
    }
}
impl Union for Limits {
    fn union(&self, other: &Self) -> Self {
        Self {
            min: self.min.min(other.min),
            max: self.max.max(other.max),
        }
    }
}

/// Represents a normalized axis, always in the range `[0, 1]`
/// Since the min and max are fixed, only the granularity is given, if known.
#[derive(Clone, Copy, Debug, Default)]
pub struct NormalizedInfo {
    pub granularity: Option<Granularity>,
}
impl Union for NormalizedInfo {
    fn union(&self, other: &Self) -> Self {
        Self {
            granularity: self.granularity.union(&other.granularity),
        }
    }
}

/// Info for an axis which may or may not correspond to a physical unit of length.
#[derive(Clone, Copy, Debug)]
pub enum LengthInfo {
    /// The axis reports a `[0, 1]` range, where the value doesn't necessarily correlate linearly with a
    /// physical unit of distance.
    Normalized(NormalizedInfo),
    /// The axis reports a physical distance in centimeters, within the range provided by
    /// [`Info::limits`].
    Centimeters(Info),
}
impl LengthInfo {
    #[must_use]
    pub fn limits(self) -> Option<Limits> {
        match self {
            Self::Normalized(_) => Some((0.0..=1.0).into()),
            Self::Centimeters(cm) => cm.limits,
        }
    }
    #[must_use]
    pub fn granularity(self) -> Option<Granularity> {
        match self {
            Self::Normalized(n) => n.granularity,
            Self::Centimeters(c) => c.granularity,
        }
    }
}
impl Union for LengthInfo {
    fn union(&self, other: &Self) -> Self {
        match (self, other) {
            (Self::Centimeters(a), Self::Centimeters(b)) => Self::Centimeters(a.union(b)),
            (Self::Normalized(a), Self::Normalized(b)) => Self::Normalized(NormalizedInfo {
                granularity: a.granularity.max(b.granularity),
            }),
            (Self::Centimeters(cm), Self::Normalized(n))
            | (Self::Normalized(n), Self::Centimeters(cm)) => {
                // Uh oh.. This occurs when a linear axis and it's sibling disagree on whether the units are normalized or not.
                // Not reallllyyyy much we can do here. Feels like an api crime. whoops!
                // FIXME!
                Self::Normalized(NormalizedInfo {
                    granularity: n.granularity.max(cm.granularity),
                })
            }
        }
    }
}
impl Default for LengthInfo {
    fn default() -> Self {
        Self::Normalized(NormalizedInfo::default())
    }
}

/// Generic information about an axis with hardware-defined limits.
#[derive(Clone, Copy, Debug, Default)]
pub struct Info {
    pub limits: Option<Limits>,
    pub granularity: Option<Granularity>,
}
impl Union for Info {
    fn union(&self, other: &Self) -> Self {
        Self {
            granularity: self.granularity.union(&other.granularity),
            limits: self.limits.union(&other.limits),
        }
    }
}

/// See [`FullInfo::position`].
#[derive(Clone, Copy, Debug, Default)]
pub struct PositionInfo {
    pub granularity: Option<PositionGranularity>,
}
impl Union for PositionInfo {
    fn union(&self, other: &Self) -> Self {
        Self {
            granularity: self.granularity.union(&other.granularity),
        }
    }
}

/// Information about a circular axis, always reporting in the range of `[0, TAU)`.
#[derive(Clone, Copy, Debug, Default)]
pub struct CircularInfo {
    pub granularity: Option<Granularity>,
}
impl Union for CircularInfo {
    fn union(&self, other: &Self) -> Self {
        Self {
            granularity: self.granularity.union(&other.granularity),
        }
    }
}

/// Information about a slider axis, always reporting in the range of `[-1, 1]` with zero being the resting point.
#[derive(Clone, Copy, Debug, Default)]
pub struct SliderInfo {
    pub granularity: Option<Granularity>,
}
impl Union for SliderInfo {
    fn union(&self, other: &Self) -> Self {
        Self {
            granularity: self.granularity.union(&other.granularity),
        }
    }
}

/// A report of the limits and capabilities of all axes, or None if the axis is
/// not supported by the device. See [`Pose`] for descriptions.
#[derive(Debug, Copy, Clone, Default)]
pub struct FullInfo {
    /// The X and Y axes - always supported, and with units of logical pixels.
    pub position: [PositionInfo; 2],
    pub slider: Option<SliderInfo>,
    pub roll: Option<CircularInfo>,
    pub pressure: Option<NormalizedInfo>,
    pub button_pressure: Option<NormalizedInfo>,
    pub tilt: Option<Info>,
    pub wheel: Option<CircularInfo>,
    pub distance: Option<LengthInfo>,
    pub contact_size: Option<LengthInfo>,
}
// I wish i wasn't so lazy that this isn't a field-wise procmacro :P
impl Union for FullInfo {
    fn union(&self, other: &Self) -> Self {
        Self {
            position: [
                self.position[0].union(&other.position[0]),
                self.position[1].union(&other.position[1]),
            ],
            slider: self.slider.union(&other.slider),
            roll: self.roll.union(&other.roll),
            pressure: self.pressure.union(&other.pressure),
            button_pressure: self.button_pressure.union(&other.button_pressure),
            tilt: self.tilt.union(&other.tilt),
            wheel: self.wheel.union(&other.wheel),
            distance: self.distance.union(&other.distance),
            contact_size: self.contact_size.union(&other.contact_size),
        }
    }
}

#[derive(thiserror::Error, Debug, Copy, Clone, PartialEq, Eq, Hash)]
#[error("axis not supported")]
/// An axis was queried that is not reported as available.
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
    /// Query the granularity of an axis. For all axis supported by this function,
    /// the granularity is the total number of states between the minimum and maximum value of the axis.
    /// # Errors
    /// `Ok(None)` if the axis is supported but does not know it's granulartiy. `Err(_)` if the unit
    /// is not supported.
    pub fn granularity(&self, axis: Axis) -> Result<Option<Granularity>, UnsupportedAxisError> {
        match axis {
            Axis::Pressure => self
                .pressure
                .map(|p| p.granularity)
                .ok_or(UnsupportedAxisError),
            Axis::ButtonPressure => self
                .button_pressure
                .map(|p| p.granularity)
                .ok_or(UnsupportedAxisError),
            Axis::Roll => self.roll.map(|p| p.granularity).ok_or(UnsupportedAxisError),
            Axis::Wheel => self
                .wheel
                .map(|p| p.granularity)
                .ok_or(UnsupportedAxisError),
            Axis::Slider => self
                .slider
                .map(|p| p.granularity)
                .ok_or(UnsupportedAxisError),
            Axis::Tilt => self
                .tilt
                .as_ref()
                .map(|p| p.granularity)
                .ok_or(UnsupportedAxisError),
            // Normalized or CM
            Axis::Distance => self
                .distance
                .map(LengthInfo::granularity)
                .ok_or(UnsupportedAxisError),
            Axis::ContactSize => self
                .contact_size
                .map(LengthInfo::granularity)
                .ok_or(UnsupportedAxisError),
        }
    }
    /// Query the limits of an axis.
    /// # Errors
    /// `Ok(None)` if the axis is supported but does not know it's range. `Err(_)` if the unit
    /// is not supported.
    pub fn limits(&self, axis: Axis) -> Result<Option<Limits>, UnsupportedAxisError> {
        // TAU.next_down(). That fn is unstable, so here it is hardcoded~!
        let tau_exclusive = f32::from_bits(0x40C9_0FDA);

        match axis {
            // ===== Fixed-range axes:
            // Normalized:
            Axis::Pressure => self
                .pressure
                .map(|_| Some((0.0..=1.0f32).into()))
                .ok_or(UnsupportedAxisError),
            Axis::ButtonPressure => self
                .button_pressure
                .map(|_| Some((0.0..=1.0f32).into()))
                .ok_or(UnsupportedAxisError),
            // Circular:
            Axis::Roll => self
                .roll
                .map(|_| Some((0.0..=tau_exclusive).into()))
                .ok_or(UnsupportedAxisError),
            Axis::Wheel => self
                .wheel
                .map(|_| Some((0.0..=tau_exclusive).into()))
                .ok_or(UnsupportedAxisError),
            // Weird guy:
            Axis::Slider => self
                .slider
                .map(|_| Some((-1.0..=1.0).into()))
                .ok_or(UnsupportedAxisError),
            // ===== Dynamic-range axes:
            Axis::Tilt => self
                .tilt
                .as_ref()
                .map(|a| a.limits)
                .ok_or(UnsupportedAxisError),
            // Normalized or CM
            Axis::Distance => self
                .distance
                .map(LengthInfo::limits)
                .ok_or(UnsupportedAxisError),
            Axis::ContactSize => self
                .contact_size
                .map(LengthInfo::limits)
                .ok_or(UnsupportedAxisError),
        }
    }
}

/// Represents the state of all axes of a tool at some snapshot in time.
///
/// Interpretations, units, and minimas/maximas of some axes require querying the [`Tool`](crate::tool::Tool) that generated this pose's [`FullInfo`].
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
    /// X Y position, in *logical pixels* from the top left of the associated window - your app must be DPI-aware for
    /// correct interpretation of these values.
    ///
    /// This may have sub-pixel precision, and may exceed your window size in the negative or positive directions.
    pub position: [f32; 2],
    /// Perpendicular distance from the surface of the tablet. This may be an arbitrary, unitless `[0, 1]` value, or
    /// reported in physical centimeters, see the [`FullInfo::distance`] of the reporting [`Tool`](crate::tool::Tool) for interpretation.
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
    /// The force on a pressure-sensitive button. See [`Pose::pressure`].
    pub button_pressure: NicheF32,
    /// Absolute tilt from perpendicular in the X and Y directions in radians. That is, the first angle
    /// describes the angle between the pen and the Z (perpendicular to the surface) axis along the XZ plane,
    /// and the second angle describes the angle between the pen and Z on the YZ plane.
    ///
    /// `[+,+]` is right+towards user, and `[-,-]` is left+away from user.
    /// # Quirks
    /// In theory the vector `[sin x, sin y]` should describe a projection of the pen's body down on the page, with length <= 1.
    /// However in practice, reported values may break this trigonometric invariant.
    pub tilt: Option<[f32; 2]>,
    /// Absolute roll in radians, around the tool's long axis. Zero is a hardware-determined "natural" angle.
    pub roll: NicheF32,
    /// Absolute scroll wheel angle and clicks in radians, unspecified range or zero-point.
    /// Note that the clicks are *not* a delta.
    pub wheel: Option<(f32, i32)>,
    /// Absolute slider position, in `[-1, 1]`, where zero is the "natural" position.
    pub slider: NicheF32,
    /// The size of the contact ellipse. First element describes the X-axis width of the ellipse,
    /// and second describes the Y-axis height. See [`FullInfo::contact_size`] of the reporting [`Tool`](crate::tool::Tool) for units.
    pub contact_size: Option<[f32; 2]>,
}
