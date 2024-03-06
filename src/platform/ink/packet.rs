use super::{core, tablet_pc, WinResult, E_FAIL, E_INVALIDARG};
use crate::{axis::Pose, util::NicheF32};

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
    pub fn read(&self, from: &mut &[i32]) -> Result<NicheF32, FilterError> {
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
pub enum Filter {
    /// Don't consume a property value and don't report any value.
    Missing,
    /// Consume the value, but report nothing.
    Skip,
    /// Consume a property value, scale, then report it.
    Read(Scaler),
}

impl Filter {
    /// Apply this filter from these properties. Will advance the slice if consumed.
    pub fn read(&self, from: &mut &[i32]) -> Result<NicheF32, FilterError> {
        match self {
            Self::Missing => Ok(NicheF32::NONE),
            Self::Skip => {
                // try consume and discard
                let trimmed = from.get(1..).ok_or(FilterError::NotEnoughData)?;
                *from = trimmed;

                Ok(NicheF32::NONE)
            }
            Self::Read(scale) => scale.read(from),
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
    pub x: Scaler,
    pub y: Scaler,
    pub normal_pressure: Filter,
    pub x_tilt: Filter,
    pub y_tilt: Filter,
    pub z: Filter,
    pub twist: Filter,
    pub button_pressure: Filter,
    pub width: Filter,
    pub height: Filter,
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
    ) -> Result<(crate::axis::Pose, Option<crate::events::FrameTimestamp>), PacketInterpretError>
    {
        // ======= ORDER IS IMPORTANT!! ========
        // If you change me, make sure to change `DESIRED_PACKET_DESCRIPTIONS` :3

        let pose = crate::axis::Pose {
            position: [
                self.x.read(&mut props)?.get().ok_or(FilterError::Value)?,
                self.y.read(&mut props)?.get().ok_or(FilterError::Value)?,
            ],
            pressure: self.normal_pressure.read(&mut props)?,
            tilt: match (
                self.x_tilt.read(&mut props)?.get(),
                self.y_tilt.read(&mut props)?.get(),
            ) {
                (None, None) => None,
                (Some(x), None) => Some([x, 0.0]),
                (None, Some(y)) => Some([0.0, y]),
                (Some(x), Some(y)) => Some([x, y]),
            },
            distance: self.z.read(&mut props)?,
            roll: self.twist.read(&mut props)?,
            button_pressure: self.button_pressure.read(&mut props)?,
            contact_size: match (
                self.width.read(&mut props)?.get(),
                self.height.read(&mut props)?.get(),
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
    pub pose: Pose,
    pub timestamp: Option<crate::events::FrameTimestamp>,
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
    /// Quirks: Sometimes zero
    resolution: f32,
    /// Quirks: *WHY DOES THE CINTIQ 16 LIST PRESSURE IN ANGULAR DEGREES*
    unit: tablet_pc::TabletPropertyMetricUnit,
}

enum RawPropertyInfo {
    /// Not supported, not in packet.
    Unsupported,
    /// Present in packet but not useful.
    Skipped,
    /// All ok!
    Ok(RawPropertyRange),
}

/// Get unit, min, max, resolution, or None if not supported.
/// # Safety
/// ¯\\\_(ツ)\_/¯
///
/// Preconditions managed by types, but calling into external code is inherently unsafe I suppose.
unsafe fn get_property_info(
    tablet: tablet_pc::IInkTablet,
    prop_guid: core::PCWSTR,
) -> WinResult<RawPropertyInfo> {
    use windows::Win32::Foundation::TPC_E_UNKNOWN_PROPERTY;
    // Would be cool to re-use this alloc (every `prop` is necessarily the same length)
    // but there isn't an api for it or.. anything else really. ough. ugh. ouch. oof. owie. heck.
    let prop = core::BSTR::from_wide(prop_guid.as_wide())?;

    if !tablet.IsPacketPropertySupported(&prop)?.as_bool() {
        // Not included, don't try to read.
        return Ok(RawPropertyInfo::Unsupported);
    }

    let mut info = RawPropertyRange::default();

    match tablet.GetPropertyMetrics(
        &prop,
        std::ptr::addr_of_mut!(info.min),
        std::ptr::addr_of_mut!(info.max),
        std::ptr::addr_of_mut!(info.unit),
        std::ptr::addr_of_mut!(info.resolution),
    ) {
        Ok(()) => Ok(RawPropertyInfo::Ok(info)),
        // Not supported by the implementor driver/hardware - still a success, just report none.
        Err(e) if e.code() == TPC_E_UNKNOWN_PROPERTY || e.code() == E_INVALIDARG => {
            println!("{prop:?} unsupported");
            // *was* reported as included, but failed to query.
            // Thus, we have to pull it from the packets, but ignore it.
            Ok(RawPropertyInfo::Skipped)
        }
        // Unhandled err, bail
        Err(e) => Err(e),
    }
}

fn normalized(info: RawPropertyRange, onto: crate::axis::Limits) -> (Filter, crate::axis::Info) {
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
            (
                Filter::Read(Scaler {
                    bias,
                    multiply: multiplier,
                }),
                crate::axis::Info {
                    granularity: Some(crate::axis::Granularity(nonzero_granularity)),
                    limits: Some(onto),
                },
            )
        } else {
            // Well that's a problem! Stems from `multiplier` being zero.
            // That's a useless scenario anyway, so skip.
            (Filter::Skip, crate::axis::Info::default())
        }
    } else {
        // Width is zero (info.min == info.max), can't divide to find the multiplier!
        // Not much we can do except skip it and report nothing.
        (Filter::Skip, crate::axis::Info::default())
    }
}

fn adapt_linear(info: RawPropertyInfo) -> (Filter, Option<crate::axis::LinearInfo>) {
    let info = match info {
        RawPropertyInfo::Unsupported => return (Filter::Missing, None),
        RawPropertyInfo::Skipped => return (Filter::Skip, None),
        RawPropertyInfo::Ok(range) => range,
    };
    let scale = match info.unit {
        tablet_pc::TPMU_Centimeters => Some(1.0),
        tablet_pc::TPMU_Inches => Some(2.54),
        // We'll just assume the `distance` wont be reported in pounds :P
        _ => None,
    };
    Scaler {
        bias: 0,
        multiply: f64::from(scale.unwrap_or(1.0) * info.resolution),
    };
    crate::axis::LinearInfo {
        unit: match scale {
            Some(_) => crate::axis::unit::Linear::Centimeters,
            None => crate::axis::unit::Linear::Unitless,
        },
        info: crate::axis::Info {
            limits: Some(crate::axis::Limits { min: (), max: () }),
            granularity: (),
        },
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
) -> WinResult<(crate::axis::FullInfo, PacketInterpreter)> {
    // Query the scale factor of a unit. If `Some`, then uses the relevant axis::unit. otherwise,
    // it is considered unitless.
    fn linear_scale_factor(unit: tablet_pc::TabletPropertyMetricUnit) -> Option<f32> {
        match unit {
            tablet_pc::TPMU_Centimeters => Some(1.0),
            tablet_pc::TPMU_Inches => Some(2.54),
            // We'll just assume the `distance` wont be reported in pounds :P
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
            // My Cintiq 16 reports NORMAL_PRESSURE in `Degrees`... good for you buddy lmao?
            _ => None,
        }
    }

    unsafe {
        println!("Querying {:?}", tablet.Name());
        // Get the position properties. Both MUST be Ok(Some)!
        let position = [
            try_property(tablet_pc::STR_GUID_X)?.ok_or(E_FAIL)?,
            try_property(tablet_pc::STR_GUID_Y)?.ok_or(E_FAIL)?,
        ];
        let pressure = try_property(tablet_pc::STR_GUID_NORMALPRESSURE)?;
        let distance = try_property(tablet_pc::STR_GUID_Z)?;
        let roll = try_property(tablet_pc::STR_GUID_TWISTORIENTATION)?;
        let button_pressure = try_property(tablet_pc::STR_GUID_BUTTONPRESSURE)?;
        // If X and/or Y tilt is supported, claim support for both with the missing axis (if any) defaulted.
        // (does one of the option adapters do this for me?)
        let tilt = match (
            try_property(tablet_pc::STR_GUID_XTILTORIENTATION)?,
            try_property(tablet_pc::STR_GUID_YTILTORIENTATION)?,
        ) {
            (Some(a), Some(b)) => Some([a, b]),
            (Some(a), None) => Some([a, Default::default()]),
            (None, Some(b)) => Some([Default::default(), b]),
            (None, None) => None,
        };
        // If X and/or Y contact size is supported, claim support for both with the missing axis (if any) defaulted.
        // (does one of the option adapters do this for me?)
        let contact_size = match (
            try_property(tablet_pc::STR_GUID_WIDTH)?,
            try_property(tablet_pc::STR_GUID_HEIGHT)?,
        ) {
            (Some(a), Some(b)) => Some([a, b]),
            (Some(a), None) => Some([a, Default::default()]),
            (None, Some(b)) => Some([Default::default(), b]),
            (None, None) => None,
        };
        // todo: use this :3
        // let caps = tablet.HardwareCapabilities()?;

        todo!()
    }
}
