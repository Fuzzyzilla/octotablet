use crate::events::raw;
use x11rb::{
    connection::{Connection, RequestConnection},
    protocol::{
        xinput::{self, ConnectionExt},
        xproto::ConnectionExt as _,
    },
};

// Note: Device Product ID (286) property of tablets states USB vid and pid, and Device Node (285)
// lists path. Impl that pleas. thanks Uwu

const XI_ALL_DEVICES: u16 = 0;
/// Magic timestamp signalling to the server "now".
const NOW_MAGIC: x11rb::protocol::xproto::Timestamp = 0;
// Strings are used to communicate the class of device, so we need a hueristic to
// find devices we are interested in and a transformation to a more well-documented enum.
// I Could not find a comprehensive guide to the strings used here.
// (I am SURE I saw one at one point, but can't find it again.) So these are just
// the ones I have access to in my testing.
/// X "device_type" atom for [`crate::tablet`]s...
const TYPE_TABLET: &str = "TABLET";
/// .. [`crate::pad`]s...
const TYPE_PAD: &str = "PAD";
/// .. [`crate::pad`]s also ?!?!?...
const TYPE_TOUCHPAD: &str = "TOUCHPAD";
/// ..stylus tips..
const TYPE_STYLUS: &str = "STYLUS";
/// and erasers!
const TYPE_ERASER: &str = "ERASER";
// Type "xwayland-pointer" is used for xwayland mice, styluses, erasers and... *squint* ...keyboards?
// The role could instead be parsed from it's user-facing device name:
// "xwayland-tablet-pad:<some value of mystery importance>"
// "xwayland-tablet eraser:<some value>" (note the hyphen becomes a space)
// "xwayland-tablet stylus:<some value>"
// Which is unfortunately a collapsed stream of all devices (similar to X's concept of a Master device)
// and thus all per-device info (names, hardware IDs, capabilities) is lost in abstraction.
const TYPE_XWAYLAND_POINTER: &str = "xwayland-pointer";

const TYPE_MOUSE: &str = "MOUSE";
const TYPE_TOUCHSCREEN: &str = "TOUCHSCREEN";

/// Comes from `xinput_open_device`. Some APIs use u16. Confusing!
pub type ID = u8;
/// Comes from datasize of "button count" field of `ButtonInfo` - button names in xinput are indices,
/// with the zeroth index referring to the tool "down" state.
pub type ButtonID = std::num::NonZero<u16>;

#[derive(Debug, Clone, Copy)]
enum ValuatorAxis {
    // Absolute position, in a normalized device space.
    // AbsX,
    // AbsY,
    AbsPressure,
    // Degrees, -,- left and away from user.
    AbsTiltX,
    AbsTiltY,
    // This pad ring, degrees, and maybe also stylus scrollwheel? I have none to test,
    // but under Xwayland this capability is listed for both pad and stylus.
    AbsWheel,
}
impl std::str::FromStr for ValuatorAxis {
    type Err = ();
    fn from_str(axis_label: &str) -> Result<Self, Self::Err> {
        Ok(match axis_label {
            // "Abs X" => Self::AbsX,
            // "Abs Y" => Self::AbsY,
            "Abs Pressure" => Self::AbsPressure,
            "Abs Tilt X" => Self::AbsTiltX,
            "Abs Tilt Y" => Self::AbsTiltY,
            "Abs Wheel" => Self::AbsWheel,
            // My guess is the next one is roll axis, but I do
            // not have a any devices that report this axis.
            _ => return Err(()),
        })
    }
}
impl From<ValuatorAxis> for crate::axis::Axis {
    fn from(value: ValuatorAxis) -> Self {
        match value {
            ValuatorAxis::AbsPressure => Self::Pressure,
            ValuatorAxis::AbsTiltX | ValuatorAxis::AbsTiltY => Self::Tilt,
            ValuatorAxis::AbsWheel => Self::Wheel,
            //Self::AbsX | Self::AbsY => return None,
        }
    }
}
enum DeviceType {
    Tool(crate::tool::Type),
    Tablet,
    Pad,
}
enum DeviceTypeOrXwayland {
    Type(DeviceType),
    /// Device type of xwayland-pointer doesn't tell us much, we must
    /// also inspect the user-facing device name.
    Xwayland,
}
impl std::str::FromStr for DeviceTypeOrXwayland {
    type Err = ();
    fn from_str(device_type: &str) -> Result<Self, Self::Err> {
        use crate::tool::Type;
        Ok(match device_type {
            TYPE_STYLUS => Self::Type(DeviceType::Tool(Type::Pen)),
            TYPE_ERASER => Self::Type(DeviceType::Tool(Type::Eraser)),
            TYPE_PAD => Self::Type(DeviceType::Pad),
            TYPE_TABLET => Self::Type(DeviceType::Tablet),
            // TYPE_MOUSE => Self::Tool(Type::Mouse),
            TYPE_XWAYLAND_POINTER => Self::Xwayland,
            _ => return Err(()),
        })
    }
}

/// Parse the device name of an xwayland device, where the type is stored.
/// Use if [`DeviceType`] parsing came back as `DeviceType::Xwayland`.
fn xwayland_type_from_name(device_name: &str) -> Option<DeviceType> {
    use crate::tool::Type;
    let class = device_name.strip_prefix("xwayland-tablet")?;
    // there is a numeric field at the end, unclear what it means.
    // For me, it's *always* `:43`, /shrug!
    let colon = class.rfind(':')?;
    let class = &class[..colon];

    Some(match class {
        // Weird inconsistent prefix xP
        "-pad" => DeviceType::Pad,
        " stylus" => DeviceType::Tool(Type::Pen),
        " eraser" => DeviceType::Tool(Type::Eraser),
        _ => return None,
    })
}

#[derive(Copy, Clone)]
enum ToolName<'a> {
    NameOnly(&'a str),
    NameAndId(&'a str, crate::tool::HardwareID),
}
impl<'a> ToolName<'a> {
    fn name(self) -> &'a str {
        match self {
            Self::NameAndId(name, _) | Self::NameOnly(name) => name,
        }
    }
    fn id(self) -> Option<crate::tool::HardwareID> {
        match self {
            Self::NameAndId(_, id) => Some(id),
            Self::NameOnly(_) => None,
        }
    }
}

/// From the user-facing Device name, try to parse a tool's hardware id.
fn tool_id_from_name(name: &str) -> ToolName {
    // X11 seems to place tool hardware IDs within the human-readable Name of the device, and this is
    // the only place it is exposed. Predictably, as with all things X, this is not documented as far
    // as I can tell. From experience, it consists of the name, a space, and a hex number (or zero)
    // in parentheses - This is a hueristic and likely non-exhaustive, Bleh.

    let try_parse = || -> Option<(&str, crate::tool::HardwareID)> {
        // Detect the range of characters within the last set of parens.
        let open_paren = name.rfind('(')?;
        let after_open_paren = open_paren + 1;
        // Find the close paren after the last open paren (weird change-of-base-address thing)
        let close_paren = after_open_paren + name.get(after_open_paren..)?.find(')')?;

        // Find the human-readable name content, minus the id field.
        let name_text = name[..open_paren].trim_ascii_end();

        // Find the id field.
        // id_text is literal '0', or a hexadecimal number prefixed by literal '0x'
        let id_text = &name[after_open_paren..close_paren];

        let id_num = if id_text == "0" {
            // Should this be considered "None"? The XP-PEN DECO-01 reports this value, despite (afaik)
            // lacking a genuine hardware ID capability.
            0
        } else if let Some(id_text) = id_text.strip_prefix("0x") {
            u64::from_str_radix(id_text, 16).ok()?
        } else {
            return None;
        };

        Some((name_text, crate::tool::HardwareID(id_num)))
    };

    if let Some((name, id)) = try_parse() {
        ToolName::NameAndId(name, id)
    } else {
        ToolName::NameOnly(name)
    }
}
/// Turn an xinput fixed-point number into a float, rounded.
// I could probably keep them fixed for more maths, but this is easy for right now.
fn fixed32_to_f32(fixed: xinput::Fp3232) -> f32 {
    // Could bit-twiddle these into place instead, likely with more precision.
    let integral = fixed.integral as f32;
    let fractional = fixed.frac as f32 / u32::MAX as f32;

    if fixed.integral.is_positive() {
        integral + fractional
    } else {
        integral - fractional
    }
}
/// Turn an xinput fixed-point number into a float, rounded.
// I could probably keep them fixed for more maths, but this is easy for right now.
fn fixed16_to_f32(fixed: i32) -> f32 {
    // Could bit-twiddle these into place instead, likely with more precision.
    (fixed as f32) / 65536.0
}

#[derive(Copy, Clone)]
enum Transform {
    BiasScale { bias: f32, scale: f32 },
}
impl Transform {
    fn transform(self, value: f32) -> f32 {
        match self {
            Self::BiasScale { bias, scale } => (value + bias) * scale,
        }
    }
    fn transform_fixed(self, value: xinput::Fp3232) -> f32 {
        self.transform(fixed32_to_f32(value))
    }
}

#[derive(Copy, Clone)]
struct AxisInfo {
    // Where in the valuator array is this?
    index: u16,
    // How to adapt the numeric value to octotablet's needs?
    transform: Transform,
}

/// Contains the metadata for translating a device's events to octotablet events.
struct ToolInfo {
    pressure: Option<AxisInfo>,
    tilt: [Option<AxisInfo>; 2],
    wheel: Option<AxisInfo>,
    /// The tablet this tool belongs to, based on heuristics.
    /// When "In" is fired, this is the device to reference, because X doesn't provide
    /// such info. If none, uses a dummy tablet.
    /// (tool -> tablet relationship is one-to-one-or-less in xinput instead of one-to-one-or-more as we expect)
    tablet: Option<ID>,
}

struct PadInfo {
    ring: Option<AxisInfo>,
}

struct BitDiff {
    bit_index: usize,
    set: bool,
}

struct BitDifferenceIter<'a> {
    from: &'a [u32],
    to: &'a [u32],
    // cursor position:
    // Which bit within the u32?
    next_bit_idx: u32,
    // Which word within the array?
    cur_word: usize,
}
impl<'a> BitDifferenceIter<'a> {
    fn diff(from: &'a [u32], to: &'a [u32]) -> Self {
        Self {
            from,
            to,
            next_bit_idx: 0,
            cur_word: 0,
        }
    }
}
impl Iterator for BitDifferenceIter<'_> {
    type Item = BitDiff;
    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let to = self.to.get(self.cur_word)?;
            let from = self.from.get(self.cur_word).copied().unwrap_or_default();
            let diff = to ^ from;

            // find the lowest set bit in the difference word.
            let next_diff_idx = self.next_bit_idx + (diff >> self.next_bit_idx).trailing_zeros();

            if next_diff_idx >= u32::BITS - 1 {
                // Advance to the next word for next go around.
                self.next_bit_idx = 0;
                self.cur_word += 1;
            } else {
                // advance cursor regularly.
                self.next_bit_idx = next_diff_idx + 1;
            }
            if next_diff_idx >= u32::BITS {
                // No bit was set in this word, skip to next word.
                continue;
            }

            // Check what the difference we just found was.
            let became_set = (to >> next_diff_idx) & 1 == 1;

            return Some(BitDiff {
                bit_index: self.cur_word * u32::BITS as usize + next_diff_idx as usize,
                set: became_set,
            });
        }
    }
}

pub struct Manager {
    conn: x11rb::rust_connection::RustConnection,
    _xinput_minor_version: u16,
    tool_infos: std::collections::BTreeMap<ID, ToolInfo>,
    open_devices: Vec<ID>,
    tools: Vec<crate::tool::Tool>,
    pad_infos: std::collections::BTreeMap<ID, PadInfo>,
    pads: Vec<crate::pad::Pad>,
    tablets: Vec<crate::tablet::Tablet>,
    events: Vec<crate::events::raw::Event<ID>>,
    window: x11rb::protocol::xproto::Window,
    atom_usb_id: Option<std::num::NonZero<x11rb::protocol::xproto::Atom>>,
    atom_device_node: Option<std::num::NonZero<x11rb::protocol::xproto::Atom>>,
}

impl Manager {
    pub fn build_window(_opts: crate::Builder, window: std::num::NonZeroU32) -> Self {
        let window = window.get();

        let (conn, _screen) = x11rb::connect(None).unwrap();
        // Check we have XInput2 and get it's version.
        conn.extension_information(xinput::X11_EXTENSION_NAME)
            .unwrap()
            .unwrap();
        let version = conn
            // What the heck is "name"? it is totally undocumented and is not part of the XLib interface.
            // I was unable to reverse engineer it, it seems to work regardless of what data is given to it.
            .xinput_get_extension_version(b"Fixme!")
            .unwrap()
            .reply()
            .unwrap();

        assert!(version.present && version.server_major >= 2);

        // conn.xinput_select_extension_event(
        //     window,
        //     // Some crazy logic involving the output of OpenDevice.
        //     // /usr/include/X11/extensions/XInput.h has the macros that do the crime, however it seems nonportable to X11rb.
        //     // https://www.x.org/archive/X11R6.8.2/doc/XGetSelectedExtensionEvents.3.html
        //     &[u32::from(xinput::CHANGE_DEVICE_NOTIFY_EVENT)],
        // )
        // .unwrap()
        // .check()
        // .unwrap();
        let hierarchy_interest = xinput::EventMask {
            deviceid: XI_ALL_DEVICES,
            mask: [
                // device add/remove/enable/disable.
                xinput::XIEventMask::HIERARCHY,
            ]
            .into(),
        };

        // Ask for notification of device added/removed/reassigned. This is done before
        // enumeration to avoid TOCTOU bug, but now the bug is in the opposite direction-
        // We could enumerate a device *and* recieve an added message for it, or get a removal
        // message for devices we never met. Beware!
        conn.xinput_xi_select_events(window, std::slice::from_ref(&hierarchy_interest))
            .unwrap()
            .check()
            .unwrap();

        // Testing with itty four button guy,
        //   TABLET = "Wacom Intuos S Pen"
        //   PAD = "Wacom Intuos S Pad"
        //   STYLUS = "Wacom Intuos S Pen Pen (0x7802cf3)" (no that isn't a typo lmao)

        // Fetch existing devices. It is important to do this after we requested to recieve `DEVICE_CHANGED` events,
        // lest we run into TOCTOU bugs!
        /*
        let mut interest = vec![];
        */

        let devices = conn.xinput_list_input_devices().unwrap().reply().unwrap();
        let mut flat_infos = &devices.infos[..];
        for (name, device) in devices.names.iter().zip(devices.devices.iter()) {
            let ty = if device.device_type != 0 {
                let mut ty = conn
                    .get_atom_name(device.device_type)
                    .unwrap()
                    .reply()
                    .unwrap()
                    .name;
                ty.push(0);
                std::ffi::CString::from_vec_with_nul(ty).ok()
            } else {
                None
            };
            if let Ok(name) = std::str::from_utf8(&name.name) {
                println!("{} - {name}", device.device_id);
            } else {
                println!("<Bad name>");
            }
            /*if ty.as_deref() == Some(TYPE_STYLUS) || ty.as_deref() == Some(TYPE_ERASER) {
                println!("^^ Binding ^^");
                //let _open = conn
                //    .xinput_open_device(device.device_id)
                //    .unwrap()
                //    .reply()
                //    .unwrap();
                //interest.push(device.device_id);
            }*/
            println!(" {ty:?} - {device:?}");
            // Take the infos for this device from the list.
            let infos = {
                let (head, tail) = flat_infos.split_at(usize::from(device.num_class_info));
                flat_infos = tail;
                head
            };

            for info in infos {
                println!(" * {info:?}");
            }
        }
        /*
        let mut interest = interest
            .into_iter()
            .map(|id| {
                xinput::EventMask {
                    deviceid: id.into(),
                    mask: [
                        // Cursor entering and leaving client area
                        xinput::XIEventMask::ENTER
                    | xinput::XIEventMask::LEAVE
                    // Barrel and tip buttons
                    | xinput::XIEventMask::BUTTON_PRESS
                    | xinput::XIEventMask::BUTTON_RELEASE
                    // Axis movement
                    | xinput::XIEventMask::MOTION,
                    ]
                    .into(),
                }
            })
            .collect::<Vec<_>>();
        interest.push(xinput::EventMask {
            deviceid: 0,
            mask: [
                // Barrel and tip buttons
                // device add/remove/capabilities changed.
                xinput::XIEventMask::HIERARCHY,
            ]
            .into(),
        });

        // Register with the server that we want to listen in on these events for all current devices:
        conn.xinput_xi_select_events(window, &interest)
            .unwrap()
            .check()
            .unwrap();*/

        // Future note for how to access core events, if needed.
        // "XSelectInput" is just a wrapper over this, funny!
        // https://github.com/mirror/libX11/blob/ff8706a5eae25b8bafce300527079f68a201d27f/src/SelInput.c#L33
        conn.change_window_attributes(
            window,
            &x11rb::protocol::xproto::ChangeWindowAttributesAux {
                event_mask: Some(x11rb::protocol::xproto::EventMask::NO_EVENT),
                ..Default::default()
            },
        )
        .unwrap()
        .check()
        .unwrap();

        let atom_usb_id = conn
            .intern_atom(false, b"Device Product ID")
            .ok()
            .and_then(|resp| resp.reply().ok())
            .and_then(|reply| reply.atom.try_into().ok());
        let atom_device_node = conn
            .intern_atom(false, b"Device Node")
            .ok()
            .and_then(|resp| resp.reply().ok())
            .and_then(|reply| reply.atom.try_into().ok());

        let mut this = Self {
            conn,
            _xinput_minor_version: version.server_minor,
            tool_infos: std::collections::BTreeMap::new(),
            pad_infos: std::collections::BTreeMap::new(),
            open_devices: vec![],
            tools: vec![],
            pads: vec![],
            events: vec![],
            tablets: vec![],
            window,
            atom_device_node,
            atom_usb_id,
        };

        // Poll for devices.
        this.repopulate();
        this
    }
    /// Close bound devices and enumerate server devices. Generates user-facing info structs and emits
    /// change events accordingly.
    #[allow(clippy::too_many_lines)]
    fn repopulate(&mut self) {
        // Fixme, hehe. We need to a) keep these alive for the next pump, and b) appropriately
        // report adds/removes.
        self.tools.clear();
        self.tablets.clear();
        self.tool_infos.clear();
        self.pad_infos.clear();

        for device in self.open_devices.drain(..) {
            self.conn
                .xinput_close_device(device)
                .unwrap()
                .check()
                .unwrap();
        }
        // Tools ids to bulk-enable events on.
        let mut tool_listen_events = vec![];

        // Okay, this is weird. There are two very similar functions, xi_query_device and list_input_devices.
        // The venne diagram of the data contained within their responses is nearly a circle, however each
        // has subtle differences such that we need to query both and join the data. >~<;
        let device_queries = self
            .conn
            .xinput_xi_query_device(XI_ALL_DEVICES)
            .unwrap()
            .reply()
            .unwrap();

        let device_list = self
            .conn
            .xinput_list_input_devices()
            .unwrap()
            .reply()
            .unwrap();

        // We recieve axis infos in a flat list, into which the individual devices refer.
        // (mutable slice as we'll trim it as we consume)
        let mut flat_infos = &device_list.infos[..];
        // We also recieve name strings in a parallel list.
        for (name, device) in device_list
            .names
            .into_iter()
            .zip(device_list.devices.into_iter())
        {
            let _infos = {
                // Split off however many axes this device claims.
                let (infos, tail_infos) = flat_infos.split_at(device.num_class_info.into());
                flat_infos = tail_infos;
                infos
            };
            // Find the query that represents this device.
            // Query and list contain very similar info, but both have tiny extra nuggets that we
            // need.
            let Some(query) = device_queries
                .infos
                .iter()
                .find(|qdevice| qdevice.deviceid == u16::from(device.device_id))
            else {
                continue;
            };

            // Query the "type" atom, which will describe what this device actually is through some heuristics.
            // We can't use the capabilities it advertises as our detection method, since a lot of them are
            // nonsensical (pad reporting absolute x,y, pressure, etc - but it doesn't do anything!)
            if device.device_type == 0 {
                // None.
                continue;
            }
            let Some(device_type) = self
                .conn
                // This is *not* cached. Should we? We expect a small set of valid values,
                // but on the other hand this isn't exactly a hot path.
                .get_atom_name(device.device_type)
                .ok()
                // Whew!
                .and_then(|response| response.reply().ok())
                .and_then(|atom| String::from_utf8(atom.name).ok())
                .and_then(|type_stirng| type_stirng.parse::<DeviceTypeOrXwayland>().ok())
            else {
                continue;
            };

            // UTF8 human-readable device name, which encodes some additional info sometimes.
            let raw_name = String::from_utf8(name.name).ok();

            let device_type = match device_type {
                DeviceTypeOrXwayland::Type(t) => t,
                // Generic xwayland type, parse the device name to find type instead.
                DeviceTypeOrXwayland::Xwayland => {
                    let Some(ty) = raw_name.as_deref().and_then(xwayland_type_from_name) else {
                        // Couldn't figure out what the device is..
                        continue;
                    };
                    ty
                }
            };

            // At this point, we're pretty sure this is a tool, pad, or tablet!

            match device_type {
                DeviceType::Tool(ty) => {
                    // It's a tool! Parse all relevant infos.

                    // Try to parse the hardware ID from the name field.
                    let name_fields = raw_name.as_deref().map(tool_id_from_name);

                    let mut octotablet_info = crate::tool::Tool {
                        internal_id: super::InternalID::XInput2(device.device_id),
                        name: name_fields.map(ToolName::name).map(ToOwned::to_owned),
                        hardware_id: name_fields.and_then(ToolName::id),
                        wacom_id: None,
                        tool_type: Some(ty),
                        axes: crate::axis::FullInfo::default(),
                    };

                    let mut x11_info = ToolInfo {
                        pressure: None,
                        tilt: [None, None],
                        wheel: None,
                        tablet: None,
                    };

                    // Look for axes!
                    for class in &query.classes {
                        if let Some(v) = class.data.as_valuator() {
                            if v.mode != xinput::ValuatorMode::ABSOLUTE {
                                continue;
                            };
                            // Weird case, that does happen in practice. :V
                            if v.min == v.max {
                                continue;
                            }
                            let Some(label) = self
                                .conn
                                .get_atom_name(v.label)
                                .ok()
                                .and_then(|response| response.reply().ok())
                                .and_then(|atom| String::from_utf8(atom.name).ok())
                                .and_then(|label| label.parse::<ValuatorAxis>().ok())
                            else {
                                continue;
                            };

                            let min = fixed32_to_f32(v.min);
                            let max = fixed32_to_f32(v.max);

                            match label {
                                ValuatorAxis::AbsPressure => {
                                    // Scale and bias to [0,1].
                                    x11_info.pressure = Some(AxisInfo {
                                        index: v.number,
                                        transform: Transform::BiasScale {
                                            bias: -min,
                                            scale: 1.0 / (max - min),
                                        },
                                    });
                                    octotablet_info.axes.pressure =
                                        Some(crate::axis::NormalizedInfo { granularity: None });
                                }
                                ValuatorAxis::AbsTiltX => {
                                    // Seemingly always in degrees.
                                    let deg_to_rad = 1.0f32.to_radians();
                                    x11_info.tilt[0] = Some(AxisInfo {
                                        index: v.number,
                                        transform: Transform::BiasScale {
                                            bias: 0.0,
                                            scale: deg_to_rad,
                                        },
                                    });

                                    let min = min.to_radians();
                                    let max = max.to_radians();

                                    let new_info = crate::axis::Info {
                                        limits: Some(crate::axis::Limits {
                                            min: min.to_radians(),
                                            max: max.to_radians(),
                                        }),
                                        granularity: None,
                                    };

                                    // Set the limits, or if already set take the union of the limits.
                                    match &mut octotablet_info.axes.tilt {
                                        slot @ None => *slot = Some(new_info),
                                        Some(v) => match &mut v.limits {
                                            slot @ None => *slot = new_info.limits,
                                            Some(v) => {
                                                v.max = v.max.max(max);
                                                v.min = v.min.min(min);
                                            }
                                        },
                                    }
                                }
                                ValuatorAxis::AbsTiltY => {
                                    // Seemingly always in degrees.
                                    let deg_to_rad = 1.0f32.to_radians();
                                    x11_info.tilt[1] = Some(AxisInfo {
                                        index: v.number,
                                        transform: Transform::BiasScale {
                                            bias: 0.0,
                                            scale: deg_to_rad,
                                        },
                                    });

                                    let min = min.to_radians();
                                    let max = max.to_radians();

                                    let new_info = crate::axis::Info {
                                        limits: Some(crate::axis::Limits {
                                            min: min.to_radians(),
                                            max: max.to_radians(),
                                        }),
                                        granularity: None,
                                    };

                                    // Set the limits, or if already set take the union of the limits.
                                    match &mut octotablet_info.axes.tilt {
                                        slot @ None => *slot = Some(new_info),
                                        Some(v) => match &mut v.limits {
                                            slot @ None => *slot = new_info.limits,
                                            Some(v) => {
                                                v.max = v.max.max(max);
                                                v.min = v.min.min(min);
                                            }
                                        },
                                    }
                                }
                                ValuatorAxis::AbsWheel => {
                                    // uhh, i don't know. I have no hardware to test with.
                                }
                            }

                            // Resolution is.. meaningless, I think. xwayland is the only server I have
                            // seen that even bothers to fill it out, and even there it's weird.
                        }
                    }

                    tool_listen_events.push(device.device_id);
                    self.tools.push(octotablet_info);
                    self.tool_infos.insert(device.device_id, x11_info);
                }
                DeviceType::Pad => {
                    let mut buttons = 0;
                    let mut ring_info = None;
                    for class in &query.classes {
                        match &class.data {
                            xinput::DeviceClassData::Button(b) => {
                                buttons = b.num_buttons();
                            }
                            xinput::DeviceClassData::Valuator(v) => {
                                // Look for and bind an "Abs Wheel" which is our ring.
                                if v.mode != xinput::ValuatorMode::ABSOLUTE {
                                    continue;
                                }
                                let Some(label) = self
                                    .conn
                                    .get_atom_name(v.label)
                                    .ok()
                                    .and_then(|response| response.reply().ok())
                                    .and_then(|atom| String::from_utf8(atom.name).ok())
                                    .and_then(|label| label.parse::<ValuatorAxis>().ok())
                                else {
                                    continue;
                                };
                                if matches!(label, ValuatorAxis::AbsWheel) {
                                    // Remap to [0, TAU], clockwise from logical north.
                                    if v.min == v.max {
                                        continue;
                                    }
                                    let min = fixed32_to_f32(v.min);
                                    let max = fixed32_to_f32(v.max);
                                    ring_info = Some(AxisInfo {
                                        index: v.number,
                                        transform: Transform::BiasScale {
                                            bias: -min,
                                            scale: std::f32::consts::TAU / (max - min),
                                        },
                                    });
                                }
                            }
                            _ => (),
                        }
                    }
                    if buttons == 0 && ring_info.is_none() {
                        // This pad has no functionality for us.
                        continue;
                    }

                    let mut rings = vec![];
                    if ring_info.is_some() {
                        rings.push(crate::pad::Ring {
                            granularity: None,
                            internal_id: crate::platform::InternalID::XInput2(device.device_id),
                        });
                    };
                    // X11 has no concept of groups (i don't .. think?)
                    // So make a single group that owns everything.
                    let group = crate::pad::Group {
                        buttons: (0..buttons).map(Into::into).collect::<Vec<_>>(),
                        feedback: None,
                        internal_id: crate::platform::InternalID::XInput2(device.device_id),
                        mode_count: None,
                        rings,
                        strips: vec![],
                    };
                    self.pads.push(crate::pad::Pad {
                        internal_id: crate::platform::InternalID::XInput2(device.device_id),
                        total_buttons: buttons.into(),
                        groups: vec![group],
                    });
                    self.pad_infos
                        .insert(device.device_id, PadInfo { ring: ring_info });
                }
                DeviceType::Tablet => {
                    // Tablets are of... dubious usefulness in xinput?
                    // They do not follow the paradigms needed by octotablet.
                    // Alas, we can still fetch some useful information!
                    let usb_id = self
                        .conn
                        // USBID consists of two 16 bit integers, [vid, pid].
                        .xinput_get_device_property(
                            self.atom_usb_id.map_or(0, std::num::NonZero::get),
                            0,
                            0,
                            2,
                            device.device_id,
                            false,
                        )
                        .ok()
                        .and_then(|resp| resp.reply().ok())
                        .and_then(|property| {
                            #[allow(clippy::get_first)]
                            // Try to accept any type.
                            Some(match property.items {
                                xinput::GetDevicePropertyItems::Data16(d) => crate::tablet::UsbId {
                                    vid: *d.get(0)?,
                                    pid: *d.get(1)?,
                                },
                                xinput::GetDevicePropertyItems::Data8(d) => crate::tablet::UsbId {
                                    vid: (*d.get(0)?).into(),
                                    pid: (*d.get(1)?).into(),
                                },
                                xinput::GetDevicePropertyItems::Data32(d) => crate::tablet::UsbId {
                                    vid: (*d.get(0)?).try_into().ok()?,
                                    pid: (*d.get(1)?).try_into().ok()?,
                                },
                                xinput::GetDevicePropertyItems::InvalidValue(_) => return None,
                            })
                        });

                    // We can also fetch device path here.

                    let tablet = crate::tablet::Tablet {
                        internal_id: super::InternalID::XInput2(device.device_id),
                        name: raw_name,
                        usb_id,
                    };

                    self.tablets.push(tablet);
                }
            }

            // If we got to this point, we accepted the device.
            // Request the server give us access to this device's events.
            // Not sure what this reply data is for.
            let repl = self
                .conn
                .xinput_open_device(device.device_id)
                .unwrap()
                .reply()
                .unwrap();
            // Keep track so we can close it later!
            self.open_devices.push(device.device_id);

            // Enable event aspects. Why is this a different process than select events?
            // Scientists are working day and night to find the answer.
            let mut enable = Vec::<(u8, u8)>::new();
            for class in repl.class_info {
                const DEVICE_KEY_PRESS: u8 = 0;
                const DEVICE_KEY_RELEASE: u8 = 1;
                const DEVICE_BUTTON_PRESS: u8 = 0;
                const DEVICE_BUTTON_RELEASE: u8 = 1;
                const DEVICE_MOTION_NOTIFY: u8 = 0;
                const DEVICE_FOCUS_IN: u8 = 0;
                const DEVICE_FOCUS_OUT: u8 = 1;
                const PROXIMITY_IN: u8 = 0;
                const PROXIMITY_OUT: u8 = 1;
                const DEVICE_STATE_NOTIFY: u8 = 0;
                const DEVICE_MAPPING_NOTIFY: u8 = 1;
                const CHANGE_DEVICE_NOTIFY: u8 = 2;
                // Reverse engineered from Xinput.h, and xinput/test.c
                // #define FindTypeAndClass(device,proximity_in_type,desired_event_mask,ProximityClass,offset) \
                // FindTypeAndClass(device, proximity_in_type, desired_event_mask, ProximityClass, _proximityIn)
                // == EXPANDED: ==
                // {
                //   int _i;
                //   XInputClassInfo *_ip;
                //   proximity_in_type = 0;
                //   desired_event_mask = 0;
                //   _i = 0;
                //   _ip = ((XDevice *) device)->classes;
                //   for (;_i< ((XDevice *) device)->num_classes; _i++, _ip++) {
                //       if (_ip->input_class == ProximityClass) {
                //           proximity_in_type = _ip->event_type_base + 0;
                //           desired_event_mask = ((XDevice *) device)->device_id << 8 | proximity_in_type;
                //       }
                //   }
                // }

                // (base, offset)

                match class.class_id {
                    // Constants taken from XInput.h
                    xinput::InputClass::PROXIMITY => {
                        enable.extend_from_slice(&[
                            (class.event_type_base, PROXIMITY_IN),
                            (class.event_type_base, PROXIMITY_OUT),
                        ]);
                    }
                    xinput::InputClass::BUTTON => {
                        enable.extend_from_slice(&[
                            (class.event_type_base, DEVICE_BUTTON_PRESS),
                            (class.event_type_base, DEVICE_BUTTON_RELEASE),
                        ]);
                    }
                    xinput::InputClass::FOCUS => {
                        enable.extend_from_slice(&[
                            (class.event_type_base, DEVICE_FOCUS_IN),
                            (class.event_type_base, DEVICE_FOCUS_OUT),
                        ]);
                    }
                    xinput::InputClass::OTHER => {
                        enable.extend_from_slice(&[
                            (class.event_type_base, DEVICE_STATE_NOTIFY),
                            (class.event_type_base, DEVICE_MAPPING_NOTIFY),
                            (class.event_type_base, CHANGE_DEVICE_NOTIFY),
                            // PROPERTY_NOTIFY
                        ]);
                    }
                    xinput::InputClass::VALUATOR => {
                        enable.push((class.event_type_base, DEVICE_MOTION_NOTIFY));
                    }
                    xinput::InputClass::KEY => {
                        enable.extend_from_slice(&[
                            (class.event_type_base, DEVICE_KEY_PRESS),
                            (class.event_type_base, DEVICE_KEY_RELEASE),
                        ]);
                    }
                    _ => (),
                }
            }
            let masks = enable
                .into_iter()
                .map(|(base, offset)| -> u32 {
                    u32::from(device.device_id) << 8 | (u32::from(base) + u32::from(offset))
                })
                .collect::<Vec<_>>();

            self.conn
                .xinput_select_extension_event(self.window, &masks)
                .unwrap()
                .check()
                .unwrap();
            /*let status = self
                .conn
                .xinput_grab_device(
                    self.window,
                    NOW_MAGIC,
                    x11rb::protocol::xproto::GrabMode::SYNC,
                    x11rb::protocol::xproto::GrabMode::SYNC,
                    false,
                    device.device_id,
                    &masks,
                )
                .unwrap()
                .reply()
                .unwrap()
                .status;

            println!("Grab {} - {:?}", device.device_id, status);*/
        }

        if !self.tools.is_empty() {
            // So.... xinput doesn't have the same "Tablet owns pads and tools"
            // hierarchy as we do. When we associate tools with tablets, we need a tablet
            // to bind it to, but xinput does not necessarily provide one.

            // Wacom tablets and the DECO-01 use a consistent naming scheme, where tools are called
            // <Tablet name> {Pen, Eraser} (hardware id), which we can use to extract such information.
            self.tablets.push(crate::tablet::Tablet {
                internal_id: super::InternalID::XInput2(0),
                name: Some("xinput master".to_owned()),
                usb_id: None,
            });
        }

        // Skip if nothing to enable. (Avoids server error)
        if tool_listen_events.is_empty() {
            return;
        }

        // Register with the server that we want to listen in on these events for all current devices:
        let interest = tool_listen_events
            .into_iter()
            .map(|id| {
                xinput::EventMask {
                    deviceid: id.into(),
                    mask: [
                        // Cursor entering and leaving client area (doesn't work lol)
                        xinput::XIEventMask::ENTER
                        | xinput::XIEventMask::LEAVE
                        // Barrel and tip buttons
                        | xinput::XIEventMask::BUTTON_PRESS
                        | xinput::XIEventMask::BUTTON_RELEASE
                        // Also enter and leave?
                        | xinput::XIEventMask::FOCUS_IN
                        | xinput::XIEventMask::FOCUS_OUT
                        // No idea, doesn't send.
                        | xinput::XIEventMask::BARRIER_HIT
                        | xinput::XIEventMask::BARRIER_LEAVE
                        // Sent when a master device is bound, and the device controlling it
                        // changes (thus presenting a master with different classes)
                        // | xinput::XIEventMask::DEVICE_CHANGED
                        | xinput::XIEventMask::PROPERTY
                        // Axis movement
                        | xinput::XIEventMask::MOTION,
                        // Proximity is implicit, i guess. I'm losing my mind.
                    ]
                    .into(),
                }
            })
            .collect::<Vec<_>>();

        self.conn
            .xinput_xi_select_events(self.window, &interest)
            .unwrap()
            .check()
            .unwrap();
    }
}

impl super::PlatformImpl for Manager {
    #[allow(clippy::too_many_lines)]
    fn pump(&mut self) -> Result<(), crate::PumpError> {
        self.events.clear();
        let mut has_repopulated = false;

        while let Ok(Some(event)) = self.conn.poll_for_event() {
            use x11rb::protocol::Event;
            match event {
                Event::XinputProximityIn(x) => {
                    // x,device_id is total garbage? what did I do to deserve this fate.-
                    self.events.push(raw::Event::Tool {
                        tool: *self.tool_infos.keys().next().unwrap(),
                        event: raw::ToolEvent::In { tablet: 0 },
                    });
                }
                Event::XinputProximityOut(x) => {
                    self.events.push(raw::Event::Tool {
                        tool: *self.tool_infos.keys().next().unwrap(),
                        event: raw::ToolEvent::Out,
                    });
                }
                // XinputDeviceButtonPress, ButtonPress, XinputRawButtonPress are red herrings.
                // Dear X consortium... What the fuck?
                Event::XinputButtonPress(e) | Event::XinputButtonRelease(e) => {
                    // Tool buttons.
                    if e.flags
                        .intersects(xinput::PointerEventFlags::POINTER_EMULATED)
                    {
                        // Key press emulation from scroll wheel.
                        continue;
                    }
                    let device_id = u8::try_from(e.deviceid).unwrap();
                    if !self.tool_infos.contains_key(&device_id) {
                        continue;
                    };

                    let button_idx = u16::try_from(e.detail).unwrap();

                    // Detail gives the "button index".
                    match button_idx {
                        // Doesn't occur, I don't think.
                        0 => (),
                        // Tip button
                        1 => {
                            if e.event_type == xinput::BUTTON_PRESS_EVENT {
                                self.events.push(raw::Event::Tool {
                                    tool: device_id,
                                    event: raw::ToolEvent::Down,
                                });
                            } else {
                                self.events.push(raw::Event::Tool {
                                    tool: device_id,
                                    event: raw::ToolEvent::Up,
                                });
                            }
                        }
                        // Other (barrel) button.
                        _ => {
                            self.events.push(raw::Event::Tool {
                                tool: device_id,
                                event: raw::ToolEvent::Button {
                                    button_id: crate::platform::ButtonID::XInput2(
                                        // Already checked != 0
                                        button_idx.try_into().unwrap(),
                                    ),
                                    pressed: e.event_type == xinput::BUTTON_PRESS_EVENT,
                                },
                            });
                        }
                    }
                }
                Event::XinputMotion(m) => {
                    // Tool valuators.
                    let mut try_uwu = || -> Option<()> {
                        let device_id = m.deviceid.try_into().ok()?;

                        let valuator_fetch = |idx: u16| -> Option<xinput::Fp3232> {
                            // Check that it's not masked out-
                            let word_idx = idx / u32::BITS as u16;
                            let bit_idx = idx % u32::BITS as u16;
                            let word = m.valuator_mask.get(usize::from(word_idx))?;

                            // This valuator did not report, value is undefined.
                            if word & (1 << bit_idx as u32) == 0 {
                                return None;
                            }

                            // Fetch it!
                            m.axisvalues.get(usize::from(idx)).copied()
                        };
                        let tool_info = self.tool_infos.get(&device_id)?;
                        // Access valuators, and map them to our range for the associated axis.
                        let pressure = tool_info
                            .pressure
                            .and_then(|axis| {
                                Some(axis.transform.transform_fixed(valuator_fetch(axis.index)?))
                            })
                            .and_then(crate::util::NicheF32::new_some)
                            .unwrap_or(crate::util::NicheF32::NONE);
                        let tilt_x = tool_info.tilt[0].and_then(|axis| {
                            Some(axis.transform.transform_fixed(valuator_fetch(axis.index)?))
                        });
                        let tilt_y = tool_info.tilt[1].and_then(|axis| {
                            Some(axis.transform.transform_fixed(valuator_fetch(axis.index)?))
                        });

                        self.events.push(raw::Event::Tool {
                            tool: device_id,
                            event: raw::ToolEvent::Pose(crate::axis::Pose {
                                // Seems to already be in logical space.
                                position: [fixed16_to_f32(m.event_x), fixed16_to_f32(m.event_y)],
                                distance: crate::util::NicheF32::NONE,
                                pressure,
                                button_pressure: crate::util::NicheF32::NONE,
                                tilt: match (tilt_x, tilt_y) {
                                    (Some(x), Some(y)) => Some([x, y]),
                                    (Some(x), None) => Some([x, 0.0]),
                                    (None, Some(y)) => Some([0.0, y]),
                                    (None, None) => None,
                                },
                                roll: crate::util::NicheF32::NONE,
                                wheel: None,
                                slider: crate::util::NicheF32::NONE,
                                contact_size: None,
                            }),
                        });
                        Some(())
                    };
                    if try_uwu().is_none() {
                        println!("failed to fetch axes.");
                    }
                }
                Event::XinputDeviceValuator(m) => {
                    // Pad valuators. Instead of the arbtrary number of valuators that the tools
                    // are sent, this sends in groups of six. Ignore all of them except the packet that
                    // contains our ring value.

                    if let Some(pad_info) = self.pad_infos.get(&m.device_id) {
                        let Some(ring_info) = pad_info.ring else {
                            continue;
                        };
                        let absolute_ring_index = ring_info.index;
                        let Some(relative_ring_indox) =
                            absolute_ring_index.checked_sub(u16::from(m.first_valuator))
                        else {
                            continue;
                        };
                        if relative_ring_indox >= m.num_valuators.into() {
                            continue;
                        }

                        let Some(&valuator_value) =
                            m.valuators.get(usize::from(relative_ring_indox))
                        else {
                            continue;
                        };

                        if valuator_value == 0 {
                            // On release, this is snapped back to zero, but zero is also a valid value. There does not
                            // seem to be a method of checking when the interaction ended to avoid this.

                            // Snapping back to zero makes this entirely useless for knob control (which is the primary
                            // purpose of the ring) so we take this little loss.
                            continue;
                        }

                        self.events.push(raw::Event::Pad {
                            pad: m.device_id,
                            event: raw::PadEvent::Group {
                                group: m.device_id,
                                event: raw::PadGroupEvent::Ring {
                                    ring: m.device_id,
                                    event: crate::events::TouchStripEvent::Pose(
                                        ring_info.transform.transform(valuator_value as f32),
                                    ),
                                },
                            },
                        });
                    }
                }
                Event::XinputDeviceButtonPress(e) | Event::XinputDeviceButtonRelease(e) => {
                    // Pad buttons.
                    let Some(pad) = self
                        .pads
                        .iter()
                        .find(|pad| *pad.internal_id.unwrap_xinput2() == e.device_id)
                    else {
                        continue;
                    };

                    let button_idx = u32::from(e.detail);
                    if button_idx == 0 || pad.total_buttons < button_idx {
                        // Okay, there's a weird off-by-one here, that even throws off the `xinput` debug
                        // utility. My Intuos Pro S reports 11 buttons, but the maximum button index is.... 11,
                        // which is clearly invalid. Silly.
                        // I interpret this as it actually being [1, max_button] instead of [0, max_button)
                        continue;
                    }

                    self.events.push(raw::Event::Pad {
                        pad: e.device_id,
                        event: raw::PadEvent::Button {
                            // Shift 1-based to 0-based indexing.
                            button_idx: button_idx - 1,
                            pressed: e.response_type == 69,
                        },
                    });
                }
                Event::XinputHierarchy(_) => {
                    // The event does not necessarily reflect *all* changes, the spec specifically says
                    // that the client should probably just rescan. lol
                    if !has_repopulated {
                        has_repopulated = true;
                        self.repopulate();
                    }
                }
                other => println!("Other: {other:?}"),
                //_ => (),
            }
        }
        Ok(())
    }
    fn raw_events(&self) -> super::RawEventsIter<'_> {
        super::RawEventsIter::XInput2(self.events.iter())
    }
    fn tablets(&self) -> &[crate::tablet::Tablet] {
        &self.tablets
    }
    fn pads(&self) -> &[crate::pad::Pad] {
        &self.pads
    }
    fn timestamp_granularity(&self) -> Option<std::time::Duration> {
        Some(std::time::Duration::from_millis(1))
    }
    fn tools(&self) -> &[crate::tool::Tool] {
        &self.tools
    }
}
