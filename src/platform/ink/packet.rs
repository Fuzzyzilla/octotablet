//! Logic for parsing the hardware-dependent packet slices.
//!
//! The packets are specified as `[i32]` but the interpretation of this varies by device.
//! The unit, ranges, precision, ect. of all of these are defined at runtime. [`Interpreter`] encodes
//! the schema for a single tablet and can be used to extract [`axis::Pose`]s from a stream.

use super::{core, tablet_pc, WinResult, E_FAIL};
use crate::{axis, events, util::NicheF32};

/// All the axes we care about ([`crate::axis::Axis`])
/// (There are Azimuth and altitude axes that can be used to derive X/Y Tilt.
/// Are there any devices that report azimuth/altitude and NOT x/y? shoud i implement that?)
pub const DESIRED_PACKET_DESCRIPTIONS: &[core::GUID] = &[
    // X, Y always reported regardless of if they're requested, and always in first and second positions
    // (Included here for capability enumeration in `query_packet_specification`)
    tablet_pc::GUID_PACKETPROPERTY_GUID_X,
    tablet_pc::GUID_PACKETPROPERTY_GUID_Y,
    // Other axes follow their indices into this list to determine where they lay in the resulting packets
    // ======== ORDER IS IMPORTANT!! =======
    // If you change me, remember to change `PropertyFilters::consume` :3
    tablet_pc::GUID_PACKETPROPERTY_GUID_NORMAL_PRESSURE,
    tablet_pc::GUID_PACKETPROPERTY_GUID_X_TILT_ORIENTATION,
    tablet_pc::GUID_PACKETPROPERTY_GUID_Y_TILT_ORIENTATION,
    tablet_pc::GUID_PACKETPROPERTY_GUID_Z,
    tablet_pc::GUID_PACKETPROPERTY_GUID_TWIST_ORIENTATION,
    tablet_pc::GUID_PACKETPROPERTY_GUID_BUTTON_PRESSURE,
    tablet_pc::GUID_PACKETPROPERTY_GUID_WIDTH,
    tablet_pc::GUID_PACKETPROPERTY_GUID_HEIGHT,
    tablet_pc::GUID_PACKETPROPERTY_GUID_TIMER_TICK,
    // Packet status always reported last regardless of it's index into this list, but still must be requested.
    // Guaranteed by the Ink API to be supported.
    tablet_pc::GUID_PACKETPROPERTY_GUID_PACKET_STATUS,
];

trait SlicePop {
    type Item;
    fn pop_front(&mut self) -> Option<Self::Item>;
}
impl<'a, T> SlicePop for &'a [T] {
    type Item = &'a T;
    fn pop_front(&mut self) -> Option<Self::Item> {
        let item = self.first()?;
        *self = &self[1..];
        Some(item)
    }
}

/// Describes the transform from raw property packet value to reported value. Works in double precision
/// due to the possibility of absurd ranges that we gotta squash down to reasonable range precisely.
/// (ie, `(value + bias) * multiplier = reported value`)
#[derive(Copy, Clone, Debug)]
pub struct Scaler {
    /// Integer bias prior to scaling, calculated precisely without overflow.
    bias: i32,
    /// Floating point multiplier.
    multiply: f64,
}
impl Scaler {
    pub fn read_from(&self, from: &mut &[i32]) -> Result<NicheF32, FilterError> {
        let Self { multiply, bias } = *self;

        let &data = from.pop_front().ok_or(FilterError::NotEnoughData)?;

        // Bias the value, expanding size to fit. Actually fits in "i33-ish"
        let biased = i64::from(data) + i64::from(bias);
        // No loss in precision! f64 can losslessly hold everything up to "i50-ish" :D
        #[allow(clippy::cast_precision_loss)]
        let biased = biased as f64;

        // Perform the transform to float. We use double precision since the
        // range of these properties could be absurd.
        let data = biased * multiply;

        // Truncate on purpose, return it if not NaN.
        #[allow(clippy::cast_possible_truncation)]
        NicheF32::new_some(data as f32).ok_or(FilterError::Value)
    }
}

/// Describes the state of an optional packet property.
#[derive(Copy, Clone, Debug)]
pub enum Tristate<T> {
    /// Don't consume a property value and don't report any value.
    NotIncluded,
    /// Consume the value, but report nothing.
    Malformed,
    /// Consume a property value, scale, then report it.
    Ok(T),
}

impl Tristate<Scaler> {
    /// Apply this filter from these properties. Will advance the slice if consumed.
    pub fn read_from(&self, from: &mut &[i32]) -> Result<NicheF32, FilterError> {
        match self {
            Self::NotIncluded => Ok(NicheF32::NONE),
            Self::Malformed => {
                from.pop_front().ok_or(FilterError::NotEnoughData)?;
                Ok(NicheF32::NONE)
            }
            Self::Ok(scale) => scale.read_from(from),
        }
    }
}
impl<T> Tristate<T> {
    /// Returns a new Tristate with the [`Tristate::Ok`] variant transformed through the
    /// given closure
    pub fn map_ok<U>(self, f: impl FnOnce(T) -> U) -> Tristate<U> {
        match self {
            Self::Ok(t) => Tristate::Ok(f(t)),
            Self::Malformed => Tristate::Malformed,
            Self::NotIncluded => Tristate::NotIncluded,
        }
    }
    /// Get the value of the `Ok` variant.
    pub fn ok(self) -> Option<T> {
        match self {
            Self::Ok(t) => Some(t),
            _ => None,
        }
    }
}

#[derive(thiserror::Error, Debug, Clone, Copy)]
pub enum FilterError {
    #[error("property slice empty")]
    NotEnoughData,
    #[error("bad value parsed")]
    Value,
}

/// Describes the state of all optional packet properties, allowing dynamically structured packets in the form of
/// `[i32]` to be interpreted as a [`axis::Pose`].
#[derive(Clone, Debug)]
pub struct Interpreter {
    pub normal_pressure: Tristate<Scaler>,
    pub tilt: [Tristate<Scaler>; 2],
    pub z: Tristate<Scaler>,
    pub twist: Tristate<Scaler>,
    pub button_pressure: Tristate<Scaler>,
    pub contact_size: [Tristate<Scaler>; 2],
    /// odd one out - always millis with no scale nor bias.
    /// true for included, false for not.
    pub timer: bool,
}

#[derive(thiserror::Error, Debug, Clone, Copy)]
pub enum InterpretError {
    #[error("property slice too long")]
    TooMuchData,
    #[error(transparent)]
    Filter(#[from] FilterError),
}

impl Interpreter {
    /// Consume the slice of properties according to these filters, producing a `Pose` and an optional timestamp.
    pub fn consume(
        &self,
        himetric_to_logical_pixels: f32,
        mut props: &[i32],
    ) -> Result<(axis::Pose, Option<crate::events::FrameTimestamp>), InterpretError> {
        // ======= ORDER IS IMPORTANT!! ========
        // If you change me, make sure to change `DESIRED_PACKET_DESCRIPTIONS` :3

        let pose = axis::Pose {
            #[allow(clippy::cast_precision_loss)]
            position: [
                *(props
                    .pop_front()
                    .ok_or(InterpretError::Filter(FilterError::NotEnoughData))?)
                    as f32
                    * himetric_to_logical_pixels,
                *(props
                    .pop_front()
                    .ok_or(InterpretError::Filter(FilterError::NotEnoughData))?)
                    as f32
                    * himetric_to_logical_pixels,
            ],
            pressure: self.normal_pressure.read_from(&mut props)?,
            tilt: match (
                self.tilt[0].read_from(&mut props)?.get(),
                self.tilt[1].read_from(&mut props)?.get(),
            ) {
                (None, None) => None,
                (Some(x), None) => Some([x, 0.0]),
                (None, Some(y)) => Some([0.0, y]),
                (Some(x), Some(y)) => Some([x, y]),
            },
            distance: self.z.read_from(&mut props)?,
            roll: self.twist.read_from(&mut props)?,
            button_pressure: self.button_pressure.read_from(&mut props)?,
            contact_size: match (
                self.contact_size[0].read_from(&mut props)?.get(),
                self.contact_size[1].read_from(&mut props)?.get(),
            ) {
                (None, None) => None,
                (Some(x), None) => Some([x, 0.0]),
                (None, Some(y)) => Some([0.0, y]),
                (Some(x), Some(y)) => Some([x, y]),
            },
            slider: NicheF32::NONE,
            wheel: None,
        };
        let timer = if self.timer {
            let &timer = props.pop_front().ok_or(FilterError::NotEnoughData)?;

            let timer = u64::try_from(timer).map_err(|_| FilterError::Value)?;
            Some(crate::events::FrameTimestamp(
                std::time::Duration::from_millis(timer),
            ))
        } else {
            None
        };
        if props.is_empty() {
            Ok((pose, timer))
        } else {
            // There's data left on the tail. This means we parsed it wrong, and things are probably
            // definitely borked about the stuff that's been parsed into `pose`, so err out.
            Err(InterpretError::TooMuchData)
        }
    }
}

bitflags::bitflags! {
    /// As described in https://learn.microsoft.com/en-us/windows/win32/tablet/packetpropertyguids-constants
    #[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
    pub struct StatusWord: i32 {
        const DOWN = 1;
        const INVERTED = 2;
        const _ = 4;
        const BARREL = 8;
    }
}

#[derive(Copy, Clone, Debug)]
pub struct Packet {
    pub pose: axis::Pose,
    pub timestamp: Option<events::FrameTimestamp>,
    pub status: StatusWord,
}

pub struct Iter<'a> {
    filters: &'a Interpreter,
    himetric_to_logical_pixel: f32,
    props: &'a [i32],
    props_per_packet: usize,
}
impl<'a> Iter<'a> {
    /// Create an iterator over the packets of a slice of data.
    pub fn new(
        filters: &'a Interpreter,
        himetric_to_logical_pixel: f32,
        props: &'a [i32],
        props_per_packet: usize,
    ) -> Self {
        // Round down to nearest whole packet
        let trim = (props.len() / props_per_packet) * props_per_packet;
        let props = &props[..trim];
        Self {
            filters,
            himetric_to_logical_pixel,
            props,
            props_per_packet,
        }
    }
}
impl<'a> Iterator for Iter<'a> {
    type Item = Result<Packet, InterpretError>;
    fn next(&mut self) -> Option<Self::Item> {
        // Get the next words...
        let packet = self.props.get(..self.props_per_packet)?;
        // Trim the words from input array... (wont panic, as above would have
        // short-circuited)
        self.props = &self.props[self.props_per_packet..];

        let &status = packet.last()?;
        // Last word is handled specially, parse the rest.
        let packet = &packet[..packet.len() - 1];

        match self.filters.consume(self.himetric_to_logical_pixel, packet) {
            Ok((pose, timestamp)) => Some(Ok(Packet {
                pose,
                timestamp,
                status: StatusWord::from_bits_truncate(status),
            })),
            Err(e) => Some(Err(e)),
        }
    }
}

/// Calculates a Limits object based on the unit's scale factor. None if a arithmetic
/// error occurs.
pub fn calc_limits(
    metrics: tablet_pc::PROPERTY_METRICS,
    scale_factor: f64,
) -> Option<axis::Limits> {
    if scale_factor.is_nan() {
        return None;
    }
    // Use f64 for exact precision on these integers
    let min = f64::from(metrics.nLogicalMin) * scale_factor;
    let max = f64::from(metrics.nLogicalMax) * scale_factor;

    // Try to convert back down to f32. Would be nice to round away-from-zero.
    #[allow(clippy::cast_possible_truncation)]
    let min = min as f32;
    #[allow(clippy::cast_possible_truncation)]
    let max = max as f32;
    if min.is_infinite() || max.is_infinite() {
        return None;
    }
    Some(axis::Limits { min, max })
}
/// Calculates the granularity based on the unit's scale factor. None if zero.
pub fn calc_granularity(metrics: tablet_pc::PROPERTY_METRICS) -> Option<axis::Granularity> {
    let granularity = metrics
        .nLogicalMax
        .abs_diff(metrics.nLogicalMin)
        // Min and Max are inclusive, diff gives us exclusive.
        .saturating_add(1);
    std::num::NonZeroU32::new(granularity).map(axis::Granularity)
}

/// Attempt to squash the range down, regardless of unit.
/// Returns [`Tristate::Malformed`] if an arithmetic error occurs and the normalization cannot
/// be performed.
fn normalized(
    metrics: tablet_pc::PROPERTY_METRICS,
    onto: axis::Limits,
) -> Tristate<(Scaler, axis::Info)> {
    // Range of integer values is a hint at precision! i64 to not overflow (real range is ~i33)
    // May be negative.
    let from_width = i64::from(metrics.nLogicalMax) - i64::from(metrics.nLogicalMin);
    // Min and Max are inclusive, magnitude +1
    let from_width = from_width + from_width.signum();

    // saturating as
    let granularity = u32::try_from(from_width.unsigned_abs()).unwrap_or(u32::MAX);

    if let Some(nonzero_granularity) = std::num::NonZeroU32::new(granularity) {
        // Width in nonzero
        // As-cast is lossless.
        #[allow(clippy::cast_precision_loss)]
        let from_width = from_width as f64;
        let onto_width = f64::from(onto.max - onto.min);
        let multiplier = onto_width / from_width;

        // Solve for bias:
        // (info.min + bias) * multiplier == onto.min
        // bias == (onto.min / multiplier) - info.min
        let bias = f64::from(onto.min) / multiplier - f64::from(metrics.nLogicalMin);

        if !bias.is_infinite() && !bias.is_nan() {
            // `as` is a saturating cast, when well-formed (checked above).
            // Do i need to handle bias < i32::MIN or bias > i32::MAX? egh.
            #[allow(clippy::cast_possible_truncation)]
            let bias = bias as i32;
            Tristate::Ok((
                Scaler {
                    bias,
                    multiply: multiplier,
                },
                axis::Info {
                    granularity: Some(axis::Granularity(nonzero_granularity)),
                    limits: Some(onto),
                },
            ))
        } else {
            // Well that's a problem! Stems from `multiplier` being zero.
            // That's a useless scenario anyway, so skip.
            Tristate::Malformed
        }
    } else {
        // Width is zero (info.min == info.max), can't divide to find the multiplier!
        // Not much we can do except skip it and report nothing.
        Tristate::Malformed
    }
}

/// Normalize a raw range into a linear unit. If unrecognized unit, fallback on a unitless normalized range.
fn linear_or_normalize(
    metrics: tablet_pc::PROPERTY_METRICS,
) -> Tristate<(Scaler, axis::LengthInfo)> {
    match linear_scale_factor(metrics.Units) {
        // Recognized unit! Report as-is and give the user some info about it.
        Some(unit_scale) => {
            // We must *also* divide by Resolution, as that tells us how many "dots per unit",
            // and then the unit scale gets us from that hardware unit to our desired unit.
            let scale = unit_scale / f64::from(metrics.fResolution);

            Tristate::Ok((
                Scaler {
                    multiply: scale,
                    bias: 0,
                },
                axis::LengthInfo::Centimeters(axis::Info {
                    granularity: calc_granularity(metrics),
                    limits: calc_limits(metrics, scale),
                }),
            ))
        }
        // Unknown unit. Normalize it to something expected :3
        None => normalized(metrics, (0.0..=1.0).into()).map_ok(|(scaler, info)| {
            (
                scaler,
                axis::LengthInfo::Normalized(axis::NormalizedInfo {
                    granularity: info.granularity,
                }),
            )
        }),
    }
}

/// Normalize a raw range into an angular unit. If unrecognized unit, fallback on a normalized range.
fn half_angle_or_normalize(metrics: tablet_pc::PROPERTY_METRICS) -> Tristate<(Scaler, axis::Info)> {
    match angular_scale_factor(metrics.Units) {
        // Recognized unit! Report as-is and give the user some info about it.
        Some(unit_scale) => {
            // We must *also* divide by Resolution, as that tells us how many "dots per unit",
            // and then the scale gets us from that unit to our desired unit.
            let scale = unit_scale / f64::from(metrics.fResolution);
            Tristate::Ok((
                Scaler {
                    // We must *also* divide by Resolution, as that tells us how many "dots per unit",
                    // and then the unit scale gets us from that hardware unit to our desired unit.
                    multiply: scale,
                    bias: 0,
                },
                axis::Info {
                    granularity: calc_granularity(metrics),
                    limits: calc_limits(metrics, scale),
                },
            ))
        }
        // Unknown unit. Normalize it to something expected :3
        None => normalized(
            metrics,
            (-std::f32::consts::PI..=std::f32::consts::PI).into(),
        ),
    }
}

fn linear_scale_factor(unit: tablet_pc::PROPERTY_UNITS) -> Option<f64> {
    // Two types for the same enum..
    let unit = tablet_pc::TabletPropertyMetricUnit(unit.0);
    match unit {
        tablet_pc::TPMU_Centimeters => Some(1.0),
        tablet_pc::TPMU_Inches => Some(2.54),
        _ => None,
    }
}
fn angular_scale_factor(unit: tablet_pc::PROPERTY_UNITS) -> Option<f64> {
    // Two types for the same enum..
    let unit = tablet_pc::TabletPropertyMetricUnit(unit.0);
    match unit {
        tablet_pc::TPMU_Radians => Some(1.0),
        tablet_pc::TPMU_Degrees => Some(1.0f64.to_radians()),
        // "Seconds" as in Arcseconds.
        tablet_pc::TPMU_Seconds => Some((1.0f64 / 3600.0f64).to_radians()),
        _ => None,
    }
}

mod util {
    pub struct OwnedCoTaskMemSlice<T> {
        ptr: *const T,
        len: usize,
    }
    impl<T> OwnedCoTaskMemSlice<T> {
        /// Transfer ownership the pointer, will be `CoTaskMemFree`'d on drop.
        /// Safety:
        /// * ptr must be allocated with `CoTaskMemAlloc`
        /// * if len is not zero, ptr must be well-aligned for T
        /// * ptr must be valid for reads for `len * sizeof T` bytes.
        pub unsafe fn new(ptr: *const T, len: usize) -> Self {
            if len != 0 {
                debug_assert!(!ptr.is_null());
                debug_assert!(ptr as usize % std::mem::align_of::<T>() == 0);
            }
            OwnedCoTaskMemSlice { ptr, len }
        }
    }
    impl<T> AsRef<[T]> for OwnedCoTaskMemSlice<T> {
        fn as_ref(&self) -> &[T] {
            if self.len == 0 {
                // self ptr is allowed to be null or unaligned if len is zero. short circuit this
                // with a known-good empty slice
                &[]
            } else {
                unsafe { std::slice::from_raw_parts(self.ptr, self.len) }
            }
        }
    }
    impl<'a, T> IntoIterator for &'a OwnedCoTaskMemSlice<T> {
        type IntoIter = std::slice::Iter<'a, T>;
        type Item = &'a T;
        fn into_iter(self) -> Self::IntoIter {
            self.as_ref().iter()
        }
    }
    impl<T> std::ops::Deref for OwnedCoTaskMemSlice<T> {
        type Target = [T];
        fn deref(&self) -> &Self::Target {
            self.as_ref()
        }
    }
    impl<T> Drop for OwnedCoTaskMemSlice<T> {
        fn drop(&mut self) {
            if !self.ptr.is_null() {
                unsafe { super::super::com::CoTaskMemFree(Some(self.ptr.cast())) };
            }
        }
    }
}

/// Query everything about a tablet needed to parse packets and convert them to [`crate::axis::Pose`]s.
/// # Safety
/// ¯\\\_(ツ)\_/¯
#[allow(clippy::too_many_lines)]
pub unsafe fn make_interpreter(
    rts: &tablet_pc::IRealTimeStylus,
    tcid: u32,
) -> WinResult<(Interpreter, axis::FullInfo)> {
    use crate::axis::Union;
    let properties = unsafe {
        let mut num_properties = 0;
        let mut properties = std::ptr::null_mut();
        rts.GetPacketDescriptionData(
            tcid,
            // We don't care about the scalefactors - they for allow going from HIMETRIC space back to
            // physical centimeters.
            None,
            None,
            std::ptr::addr_of_mut!(num_properties),
            std::ptr::addr_of_mut!(properties),
        )?;
        // Too many properties. Must be a subset of desired descriptions.
        if num_properties > u32::try_from(DESIRED_PACKET_DESCRIPTIONS.len()).unwrap() {
            // Short-circuiting before mem management is set up, just do it ourselves!
            super::com::CoTaskMemFree(Some(properties.cast()));
            return Err(E_FAIL.into());
        }
        // Place it in a wrapper to free mem on drop!
        util::OwnedCoTaskMemSlice::new(
            properties,
            // As ok, checked above.
            num_properties as usize,
        )
    };

    // We need to find which of `DESIRED_PACKET_DESCRIPTIONS` is actually reflected in `properties`.
    // Guaranteed to be the same order and to be a subset, but some may be missing.

    // Sanity check: Ensure they're all members of DESIRED_PACKET_DESC and in the right order.
    // (Also ensures they're all unique!). guaranteed by Ink api.
    let mut desired_idx = 0;
    for property in &properties {
        // Scan forward until found
        while property.guid != DESIRED_PACKET_DESCRIPTIONS[desired_idx] {
            desired_idx += 1;
            if desired_idx == DESIRED_PACKET_DESCRIPTIONS.len() {
                // UH OH! Either the tablet properties are not a subset of `DESIRED_PACKET_DESCRIPTIONS` or
                // they're in the wrong order. Either way that's an invalid API result.
                return Err(E_FAIL.into());
            }
        }
    }

    // more sanity check:
    // Check required and guaranteed to be supported properties, X Y and STATUS
    if properties.len() < 3
        || properties[0].guid != DESIRED_PACKET_DESCRIPTIONS[0]
        || properties[1].guid != DESIRED_PACKET_DESCRIPTIONS[1]
        || properties.last().unwrap().guid != *DESIRED_PACKET_DESCRIPTIONS.last().unwrap()
    {
        return Err(E_FAIL.into());
    }

    let mut interpreter = Interpreter {
        normal_pressure: Tristate::NotIncluded,
        tilt: [Tristate::NotIncluded; 2],
        z: Tristate::NotIncluded,
        twist: Tristate::NotIncluded,
        button_pressure: Tristate::NotIncluded,
        contact_size: [Tristate::NotIncluded; 2],
        timer: false,
    };
    let mut info = axis::FullInfo::default();

    // Cut out first two and last one, those are X,Y, .., STATUS which we have no need to query.
    // TODO: X,Y granularity calculation. Bleh, left as None for now.
    for prop in &properties[2..properties.len() - 1] {
        match prop.guid {
            tablet_pc::GUID_PACKETPROPERTY_GUID_NORMAL_PRESSURE => {
                let norm = normalized(prop.PropertyMetrics, (0.0..=1.0).into());

                interpreter.normal_pressure = norm.map_ok(|(a, _)| a);
                info.pressure = norm.ok().map(|(_, b)| axis::NormalizedInfo {
                    granularity: b.granularity,
                });
            }
            tablet_pc::GUID_PACKETPROPERTY_GUID_BUTTON_PRESSURE => {
                let norm = normalized(prop.PropertyMetrics, (0.0..=1.0).into());

                interpreter.button_pressure = norm.map_ok(|(a, _)| a);
                info.button_pressure = norm.ok().map(|(_, b)| axis::NormalizedInfo {
                    granularity: b.granularity,
                });
            }

            tablet_pc::GUID_PACKETPROPERTY_GUID_X_TILT_ORIENTATION
            | tablet_pc::GUID_PACKETPROPERTY_GUID_Y_TILT_ORIENTATION => {
                let norm = half_angle_or_normalize(prop.PropertyMetrics);

                match prop.guid {
                    tablet_pc::GUID_PACKETPROPERTY_GUID_X_TILT_ORIENTATION => {
                        interpreter.tilt[0] = norm.map_ok(|(a, _)| a);
                    }
                    tablet_pc::GUID_PACKETPROPERTY_GUID_Y_TILT_ORIENTATION => {
                        interpreter.tilt[1] = norm.map_ok(|(a, _)| a);
                    }
                    _ => unreachable!(),
                }

                if let Some(new_info) = norm.ok().map(|(_, b)| b) {
                    // Extend the tilt with our new one.
                    info.tilt = match info.tilt {
                        // Already had, take the "best" properties of both.
                        Some(t) => Some(t.union(&new_info)),
                        // Didn't have, replace.
                        None => Some(new_info),
                    };
                }
            }
            tablet_pc::GUID_PACKETPROPERTY_GUID_TWIST_ORIENTATION => {
                // TAU.next_down().
                let tau_exclusive = f32::from_bits(0x40C9_0FDA);

                let norm = normalized(prop.PropertyMetrics, (0.0..=tau_exclusive).into());

                interpreter.twist = norm.map_ok(|(a, _)| a);
                info.roll = norm.ok().map(|(_, b)| axis::CircularInfo {
                    granularity: b.granularity,
                });
            }
            tablet_pc::GUID_PACKETPROPERTY_GUID_Z => {
                let norm = linear_or_normalize(prop.PropertyMetrics);

                interpreter.z = norm.map_ok(|(a, _)| a);
                info.distance = norm.ok().map(|(_, b)| b);
            }
            tablet_pc::GUID_PACKETPROPERTY_GUID_WIDTH
            | tablet_pc::GUID_PACKETPROPERTY_GUID_HEIGHT => {
                let norm = linear_or_normalize(prop.PropertyMetrics);

                match prop.guid {
                    tablet_pc::GUID_PACKETPROPERTY_GUID_WIDTH => {
                        interpreter.contact_size[0] = norm.map_ok(|(a, _)| a);
                    }
                    tablet_pc::GUID_PACKETPROPERTY_GUID_HEIGHT => {
                        interpreter.contact_size[1] = norm.map_ok(|(a, _)| a);
                    }
                    _ => unreachable!(),
                }

                if let Some(new_info) = norm.ok().map(|(_, b)| b) {
                    // Extend the tilt with our new one.
                    info.contact_size = match info.contact_size {
                        // Already had, take the "best" properties of both.
                        Some(t) => Some(t.union(&new_info)),
                        // Didn't have, replace.
                        None => Some(new_info),
                    };
                }
            }
            tablet_pc::GUID_PACKETPROPERTY_GUID_TIMER_TICK => {
                interpreter.timer = true;
            }
            // Sanity check above ensured that this is a subset of DESIRED_PACKET_DESCRIPTIONS,
            // where these ^^ patterns are from.
            _ => unreachable!(),
        }
    }

    Ok((interpreter, info))
}
