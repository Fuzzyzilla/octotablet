use super::{core, tablet_pc, WinResult, E_FAIL, E_INVALIDARG};
use crate::{axis, events, util::NicheF32};

/// All the axes we care about ([`crate::tool::Axis`])
/// (There are Azimuth and altitude axes that can be used to derive X/Y Tilt.
/// Are there any devices that report azimuth/altitude and NOT x/y? shoud i implement that?)
pub const DESIRED_PACKET_DESCRIPTIONS: &[core::GUID] = &[
    // X, Y always reported regardless of if they're requested, and always in first and second positions
    // tablet_pc::GUID_PACKETPROPERTY_GUID_X,
    // tablet_pc::GUID_PACKETPROPERTY_GUID_Y,
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
    tablet_pc::GUID_PACKETPROPERTY_GUID_PACKET_STATUS,
];

/// Describes the transform from raw property packet value to reported value. Works in double precision
/// due to the possibility of absurd ranges that we gotta squash down to reasonable range precisely.
/// (ie, `(value + bias) * multiplier = reported value`)
#[derive(Copy, Clone)]
pub struct Scaler {
    /// Integer bias prior to scaling, calculated precisely without overflow.
    bias: i32,
    /// Floating point multiplier.
    multiply: f64,
}
impl Scaler {
    pub fn read_from(&self, from: &mut &[i32]) -> Result<NicheF32, FilterError> {
        let Self { multiply, bias } = *self;

        let &data = from.first().ok_or(FilterError::NotEnoughData)?;
        // Advance the slice - infallible since get(..) woulda bailed!
        *from = &from[1..];

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
#[derive(Copy, Clone)]
pub enum Tristate<T> {
    /// Don't consume a property value and don't report any value.
    Missing,
    /// Consume the value, but report nothing.
    Skip,
    /// Consume a property value, scale, then report it.
    Read(T),
}

impl Tristate<Scaler> {
    /// Apply this filter from these properties. Will advance the slice if consumed.
    pub fn read_from(&self, from: &mut &[i32]) -> Result<NicheF32, FilterError> {
        match self {
            Self::Missing => Ok(NicheF32::NONE),
            Self::Skip => {
                // try consume and discard
                let trimmed = from.get(1..).ok_or(FilterError::NotEnoughData)?;
                *from = trimmed;

                Ok(NicheF32::NONE)
            }
            Self::Read(scale) => scale.read_from(from),
        }
    }
}
impl<T> Tristate<T> {
    /// Returns a new Tristate with the [`Tristate::Read`] variant transformed through the
    /// given closure
    pub fn map_read<U>(self, f: impl FnOnce(T) -> U) -> Tristate<U> {
        match self {
            Self::Read(t) => Tristate::Read(f(t)),
            Self::Skip => Tristate::Skip,
            Self::Missing => Tristate::Missing,
        }
    }
    /// Returns a new Tristate with the [`Tristate::Read`] variant transformed into a new Tristate through the
    /// given closure. See [`Option::and_then`]
    pub fn and_then<U>(self, f: impl FnOnce(T) -> Tristate<U>) -> Tristate<U> {
        match self {
            Self::Read(t) => f(t),
            Self::Skip => Tristate::Skip,
            Self::Missing => Tristate::Missing,
        }
    }
    /// Get the value of the `Read` variant.
    pub fn read(self) -> Option<T> {
        match self {
            Self::Read(t) => Some(t),
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
/// `[i32]` to be interpreted.
#[derive(Clone)]
pub struct PacketInterpreter {
    pub position: [Scaler; 2],
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
pub enum PacketInterpretError {
    #[error("property slice too long")]
    TooMuchData,
    #[error(transparent)]
    Filter(#[from] FilterError),
}

impl PacketInterpreter {
    /// Consume the slice of properties according to these filters, producing a `Pose` and an optional timestamp.
    pub fn consume(
        &self,
        mut props: &[i32],
    ) -> Result<(axis::Pose, Option<crate::events::FrameTimestamp>), PacketInterpretError> {
        // ======= ORDER IS IMPORTANT!! ========
        // If you change me, make sure to change `DESIRED_PACKET_DESCRIPTIONS` :3

        let pose = axis::Pose {
            position: [
                self.position[0]
                    .read_from(&mut props)?
                    .get()
                    .ok_or(FilterError::Value)?,
                self.position[1]
                    .read_from(&mut props)?
                    .get()
                    .ok_or(FilterError::Value)?,
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
            let &timer = props.first().ok_or(FilterError::NotEnoughData)?;
            // Advance the slice - infallible since get(..) woulda bailed!
            props = &props[1..];

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
            Err(PacketInterpretError::TooMuchData)
        }
    }
}

#[derive(Copy, Clone, Debug)]
pub struct Packet {
    pub pose: axis::Pose,
    pub timestamp: Option<events::FrameTimestamp>,
    pub status: i32,
}
pub struct PacketIter<'a> {
    filters: &'a PacketInterpreter,
    props: &'a [i32],
    props_per_packet: usize,
}
impl<'a> PacketIter<'a> {
    /// Create an iterator over the packets of a slice of data.
    pub fn new(filters: &'a PacketInterpreter, props: &'a [i32], props_per_packet: usize) -> Self {
        // Round down to nearest whole packet
        let trim = (props.len() / props_per_packet) * props_per_packet;
        let props = &props[..trim];
        Self {
            filters,
            props,
            props_per_packet,
        }
    }
}
impl<'a> Iterator for PacketIter<'a> {
    type Item = Result<Packet, PacketInterpretError>;
    fn next(&mut self) -> Option<Self::Item> {
        // Get the next words...
        let packet = self.props.get(..self.props_per_packet)?;
        // Trim the words from input array... (wont panic, as above would have
        // short-circuited)
        self.props = &self.props[self.props_per_packet..];

        let &status = packet.last()?;
        let packet = &packet[..packet.len() - 1];

        match self.filters.consume(packet) {
            Ok((pose, timestamp)) => Some(Ok(Packet {
                pose,
                timestamp,
                status,
            })),
            Err(e) => Some(Err(e)),
        }
    }
}

#[derive(Clone, Copy, Default)]
struct RawPropertyRange {
    /// Quirks: Sometimes < 0 for units where that's meaningless
    min: i32,
    max: i32,
    /// Resultion, in points per <unit>
    /// (so, resolution = 400, unit = inches, means 400 dpi)
    /// Quirks: Sometimes zero
    resolution: f32,
    /// Quirks: *WHY DOES THE CINTIQ 16 LIST PRESSURE IN ANGULAR DEGREES*
    unit: tablet_pc::TabletPropertyMetricUnit,
}
impl RawPropertyRange {
    /// Calculates a Limits object based on the unit's scale factor. None if a arithmetic
    /// error occurs.
    pub fn calc_limits(self, scale_factor: f32) -> Option<axis::Limits> {
        if scale_factor.is_nan() {
            return None;
        }
        // Use f64 for exact precision on these integers
        let scale_factor = f64::from(scale_factor);
        let min = f64::from(self.min) * scale_factor;
        let max = f64::from(self.max) * scale_factor;

        // Try to convert back down to f32.
        let min = min as f32;
        let max = max as f32;
        if min.is_infinite() || max.is_infinite() {
            return None;
        }
        Some(axis::Limits { min, max })
    }
    /// Calculates the granularity based on the unit's scale factor. None if a arithmetic
    /// error occurs.
    pub fn calc_granularity(self, scale_factor: f32) -> Option<axis::Granularity> {
        // Divide here, since the resolution is inversely related to the size of the unit.
        // `(1 dots/inch) -> (1/2.45 dots/cm)`
        let resolution = self.resolution / scale_factor;
        // Saturate. Negative/NaN becomes 0 (invalid), over gets clamped down. Ok!!
        let granularity = resolution.ceil() as u32;
        std::num::NonZeroU32::new(granularity).map(axis::Granularity)
    }
}

/// Get unit, min, max, resolution, or None if not supported.
/// # Safety
/// ¯\\\_(ツ)\_/¯
///
/// Preconditions managed by types, but calling into external code is inherently unsafe I suppose.
unsafe fn get_property_info(
    tablet: tablet_pc::IInkTablet,
    prop_guid: core::PCWSTR,
) -> WinResult<Tristate<RawPropertyRange>> {
    unsafe {
        use windows::Win32::Foundation::TPC_E_UNKNOWN_PROPERTY;
        // Would be cool to re-use this alloc (every `prop` is necessarily the same length)
        // but there isn't an api for it or.. anything else really. ough. ugh. ouch. oof. owie. heck.
        let prop = core::BSTR::from_wide(prop_guid.as_wide())?;

        if !tablet.IsPacketPropertySupported(&prop)?.as_bool() {
            // Not included, don't try to read.
            return Ok(Tristate::Missing);
        }

        let mut info = RawPropertyRange::default();

        match tablet.GetPropertyMetrics(
            &prop,
            std::ptr::addr_of_mut!(info.min),
            std::ptr::addr_of_mut!(info.max),
            std::ptr::addr_of_mut!(info.unit),
            std::ptr::addr_of_mut!(info.resolution),
        ) {
            Ok(()) => Ok(Tristate::Read(info)),
            // Not supported by the implementor driver/hardware - still a success, just report none.
            Err(e) if e.code() == TPC_E_UNKNOWN_PROPERTY || e.code() == E_INVALIDARG => {
                println!("{prop:?} unsupported");
                // *was* reported as included, but failed to query.
                // Thus, we have to pull it from the packets, but ignore it.
                Ok(Tristate::Skip)
            }
            // Unhandled err, bail
            Err(e) => Err(e),
        }
    }
}

fn normalized(info: RawPropertyRange, onto: axis::Limits) -> Tristate<(Scaler, axis::Info)> {
    // Range of integer values is a hint at precision! i64 to not overflow (real range is ~i33)
    let from_width = i64::from(info.max) - i64::from(info.min);
    // saturating as
    let granularity = u32::try_from(from_width.unsigned_abs()).unwrap_or(u32::MAX);

    if let Some(nonzero_granularity) = std::num::NonZeroU32::new(granularity) {
        // Width in nonzero
        // As-cast is lossless.
        let from_width = from_width as f64;
        let onto_width = f64::from(onto.max - onto.min);
        let multiplier = onto_width / from_width;

        // Solve for bias:
        // (info.min + bias) * multiplier == onto.min
        // bias == (onto.min / multiplier) - info.min
        let bias = f64::from(onto.min) / multiplier - f64::from(info.min);

        if !bias.is_infinite() && !bias.is_nan() {
            // `as` is a saturating cast, when well-formed (checked above).
            // Do i need to handle bias < i32::MIN or bias > i32::MAX? egh.
            let bias = bias as i32;
            Tristate::Read((
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
            Tristate::Skip
        }
    } else {
        // Width is zero (info.min == info.max), can't divide to find the multiplier!
        // Not much we can do except skip it and report nothing.
        Tristate::Skip
    }
}

/// Normalize a raw range into a linear unit. If unrecognized unit, fallback on a unitless normalized range.
fn linear_or_normalize(
    info: RawPropertyRange,
    fallback_limits: axis::Limits,
) -> Tristate<(Scaler, axis::LinearInfo)> {
    match linear_scale_factor(info.unit) {
        // Recognized unit! Report as-is and give the user some info about it.
        Some(scale) => Tristate::Read((
            Scaler {
                multiply: f64::from(scale),
                bias: 0,
            },
            axis::LinearInfo {
                unit: axis::unit::Linear::Centimeters,
                info: axis::Info {
                    granularity: info.calc_granularity(scale),
                    limits: info.calc_limits(scale),
                },
            },
        )),
        // Unknown unit. Normalize it to something expected :3
        None => normalized(info, fallback_limits).map_read(|(scaler, info)| {
            (
                scaler,
                axis::LinearInfo {
                    unit: axis::unit::Linear::Unitless,
                    info,
                },
            )
        }),
    }
}
/// Normalize a raw range into an angular unit. If unrecognized unit, fallback on a normalized range.
fn angular_or_normalize(
    info: RawPropertyRange,
    fallback_limits: axis::Limits,
    fallback_unit: axis::unit::Angle,
) -> Tristate<(Scaler, axis::AngleInfo)> {
    match angular_scale_factor(info.unit) {
        // Recognized unit! Report as-is and give the user some info about it.
        Some(scale) => Tristate::Read((
            Scaler {
                multiply: f64::from(scale),
                bias: 0,
            },
            axis::AngleInfo {
                unit: axis::unit::Angle::Radians,
                info: axis::Info {
                    granularity: info.calc_granularity(scale),
                    limits: info.calc_limits(scale),
                },
            },
        )),
        // Unknown unit. Normalize it to something expected :3
        None => normalized(info, fallback_limits).map_read(|(scaler, info)| {
            (
                scaler,
                axis::AngleInfo {
                    unit: fallback_unit,
                    info,
                },
            )
        }),
    }
}
/// Normalize a raw range into an angular unit. If unrecognized unit, fallback on a unitless normalized range.
fn force_or_normalize(
    info: RawPropertyRange,
    fallback_limits: axis::Limits,
) -> Tristate<(Scaler, axis::ForceInfo)> {
    match force_scale_factor(info.unit) {
        // Recognized unit! Report as-is and give the user some info about it.
        Some(scale) => Tristate::Read((
            Scaler {
                multiply: f64::from(scale),
                bias: 0,
            },
            axis::ForceInfo {
                unit: axis::unit::Force::Grams,
                info: axis::Info {
                    granularity: info.calc_granularity(scale),
                    limits: info.calc_limits(scale),
                },
            },
        )),
        // Unknown unit. Normalize it to something expected :3
        None => normalized(info, fallback_limits).map_read(|(scaler, info)| {
            (
                scaler,
                axis::ForceInfo {
                    unit: axis::unit::Force::Unitless,
                    info,
                },
            )
        }),
    }
}
fn linear_scale_factor(unit: tablet_pc::TabletPropertyMetricUnit) -> Option<f32> {
    match unit {
        tablet_pc::TPMU_Centimeters => Some(1.0),
        tablet_pc::TPMU_Inches => Some(2.54),
        _ => None,
    }
}
fn angular_scale_factor(unit: tablet_pc::TabletPropertyMetricUnit) -> Option<f32> {
    match unit {
        tablet_pc::TPMU_Radians => Some(1.0),
        tablet_pc::TPMU_Degrees => Some(1.0f32.to_radians()),
        tablet_pc::TPMU_Seconds => Some((1.0f32 / 3600.0f32).to_radians()),
        _ => None,
    }
}
fn force_scale_factor(unit: tablet_pc::TabletPropertyMetricUnit) -> Option<f32> {
    match unit {
        tablet_pc::TPMU_Grams => Some(1.0),
        tablet_pc::TPMU_Pounds => Some(453.59237),
        _ => None,
    }
}

/// Query everything about a tablet needed to parse packets and convert them to [`Pose`]s
///
/// # Errors
/// All errors from `GetPropertyMetrics` that are not `TPC_E_UNKNOWN_PROPERTY` are forwarded.
/// # Safety
/// ¯\\\_(ツ)\_/¯
unsafe fn query_packet_specification(
    tablet: tablet_pc::IInkTablet,
) -> WinResult<(PacketInterpreter, axis::FullInfo)> {
    unsafe {
        println!("Querying {:?}", tablet.Name());
        // Normalize for force, 0..1
        let normalize_force = |r: RawPropertyRange| force_or_normalize(r, (0.0..=1.0).into());
        // Normalize for tilts, -PI..PI
        let normalize_half_angle = |r: RawPropertyRange| {
            angular_or_normalize(
                r,
                (-std::f32::consts::PI..=std::f32::consts::PI).into(),
                axis::unit::Angle::Radians,
            )
        };
        // Normalize for rotations, 0..TAU
        let normalize_full_angle = |r: RawPropertyRange| {
            angular_or_normalize(
                r,
                (0.0..=std::f32::consts::TAU).into(),
                axis::unit::Angle::Radians,
            )
        };
        // Normalize for distances, 0..1
        let normalize_linear = |r: RawPropertyRange| linear_or_normalize(r, (0.0..=1.0).into());

        // Get the position properties. Both MUST be included and not skipped!
        // Problem: We cannot query the relationship between these units and logical or phsical
        // screen pixels. As far as I understand, we must determine empirically, oof!
        let position = [
            get_property_info(tablet.clone(), tablet_pc::STR_GUID_X)?
                .read()
                .ok_or(E_FAIL)?,
            get_property_info(tablet.clone(), tablet_pc::STR_GUID_Y)?
                .read()
                .ok_or(E_FAIL)?,
        ];

        let normal_pressure =
            get_property_info(tablet.clone(), tablet_pc::STR_GUID_NORMALPRESSURE)?
                .and_then(normalize_force);
        let z = get_property_info(tablet.clone(), tablet_pc::STR_GUID_Z)?
            .and_then(|r| linear_or_normalize(r, (0.0..=1.0).into()));
        let twist = get_property_info(tablet.clone(), tablet_pc::STR_GUID_TWISTORIENTATION)?
            .and_then(normalize_full_angle);
        let button_pressure =
            get_property_info(tablet.clone(), tablet_pc::STR_GUID_BUTTONPRESSURE)?
                .and_then(|r| force_or_normalize(r, (0.0..=1.0).into()));

        // If X and/or Y tilt is supported, claim support for both with the missing axis (if any) defaulted.
        // (does one of the option adapters do this for me?)
        let tilt = (
            get_property_info(tablet.clone(), tablet_pc::STR_GUID_XTILTORIENTATION)?
                .and_then(normalize_half_angle),
            get_property_info(tablet.clone(), tablet_pc::STR_GUID_YTILTORIENTATION)?
                .and_then(normalize_half_angle),
        );

        // If X and/or Y contact size is supported, claim support for both with the missing axis (if any) defaulted.
        // (does one of the option adapters do this for me?)
        let contact_size = (
            get_property_info(tablet.clone(), tablet_pc::STR_GUID_WIDTH)?
                .and_then(normalize_linear),
            get_property_info(tablet.clone(), tablet_pc::STR_GUID_HEIGHT)?
                .and_then(normalize_linear),
        );

        // The value if the `RawPropertyRange` here is meaningless to us, we care only if it's
        // reported at all.
        let timer = matches!(
            get_property_info(tablet, tablet_pc::STR_GUID_TIMERTICK)?,
            Tristate::Read(_) | Tristate::Skip
        );

        // todo: use this :3
        // let caps = tablet.HardwareCapabilities()?;

        Ok((
            PacketInterpreter {
                position: [Scaler {
                    multiply: 0.0,
                    bias: 0,
                }; 2],
                z: z.map_read(|(a, _)| a),
                tilt: todo!(),
                twist: twist.map_read(|(a, _)| a),
                normal_pressure: normal_pressure.map_read(|(a, _)| a),
                button_pressure: button_pressure.map_read(|(a, _)| a),
                contact_size: todo!(),
                timer,
            },
            axis::FullInfo {
                position: [axis::Info::default(); 2],
                distance: z.read().map(|(_, b)| b),
                pressure: normal_pressure.read().map(|(_, b)| b),
                button_pressure: button_pressure.read().map(|(_, b)| b),
                tilt: todo!(),
                // Required to be radians, so we discard the unit.
                roll: twist.read().map(|(_, b)| b.info),
                contact_size: todo!(),
                // Unsupported by the Ink RTS api
                wheel: None,
                slider: None,
            },
        ))
    }
}
