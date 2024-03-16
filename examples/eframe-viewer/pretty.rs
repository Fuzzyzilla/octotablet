//! Pretty-printing of events.
//!
use std::fmt::Debug;

use octotablet::events::{Event, PadEvent, PadGroupEvent, TabletEvent, ToolEvent, TouchStripEvent};

pub mod colors {
    use eframe::egui::Color32;
    pub const ADDED: Color32 = Color32::GREEN;
    pub const REMOVED: Color32 = Color32::RED;

    pub const PRESSED: Color32 = Color32::DARK_GREEN;
    pub const RELEASED: Color32 = Color32::DARK_RED;

    pub const ENTER: Color32 = Color32::YELLOW;
    pub const EXIT: Color32 = Color32::BROWN;

    pub const POSE: Color32 = Color32::LIGHT_GRAY;
    pub const SOURCE: Color32 = Color32::LIGHT_GRAY;
    pub const TIME: Color32 = Color32::GRAY;

    pub const MODE: Color32 = Color32::LIGHT_BLUE;
}
/// Wrapper to make "Some(<formattable>)" and "None" look nicer.
/// Displays instead as the value, otherwise "Unknown" followed by a noun.
#[derive(Clone, Copy, Default)]
struct FormatInOption<T> {
    pub value: Option<T>,
    pub fallback_noun: &'static str,
}
impl<T> FormatInOption<T> {
    /// When value is None, display as "Unknown <fallback>"
    pub fn with_noun(value: Option<T>, fallback_noun: &'static str) -> Self {
        Self {
            value,
            fallback_noun,
        }
    }
}
impl<T: std::fmt::Debug> std::fmt::Debug for FormatInOption<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self { value: Some(t), .. } => t.fmt(f),
            Self {
                value: None,
                fallback_noun,
            } => write!(f, "Unknown {fallback_noun}"),
        }
    }
}
impl<T: std::fmt::Display> std::fmt::Display for FormatInOption<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self { value: Some(t), .. } => t.fmt(f),
            Self {
                value: None,
                fallback_noun,
            } => write!(f, "Unknown {fallback_noun}"),
        }
    }
}

/// Create a unique tool name.
pub fn name_tool(tool: &octotablet::tool::Tool) -> String {
    // Human readable title (Name, type, or none)
    let ty: Option<&str> = tool
        .name
        .as_deref()
        .or(tool.tool_type.as_ref().map(|ty| -> &str { ty.as_ref() }));
    // Names aren't unique, so give a differentiator:
    // Unique ID (Hard id or local ID)
    let local_id = tool.id();
    let id: &dyn Debug = tool
        .hardware_id
        .as_ref()
        .map(|hid| hid as &dyn Debug)
        .unwrap_or_else(|| &local_id as &dyn Debug);

    format!("{} ({:08X?})", FormatInOption::with_noun(ty, "Tool"), id)
}

/// Create a unique tablet name.
pub fn name_tablet(tablet: &octotablet::tablet::Tablet) -> String {
    format!(
        "{} ({:08X?})",
        FormatInOption::with_noun(tablet.name.as_ref(), "Tablet"),
        tablet.id()
    )
}
/// Create a unique pad name.
pub fn name_pad(pad: &octotablet::pad::Pad) -> String {
    format!("Pad {:08X?}", pad.id())
}

/// Uses a USB ID database to fetch info strings
pub fn format_usb_id(id: Option<octotablet::tablet::UsbId>) -> String {
    use usb_ids::FromId;
    if let Some(id @ octotablet::tablet::UsbId { vid, pid }) = id {
        match usb_ids::Device::from_vid_pid(vid, pid) {
            Some(device) => {
                format!(
                    "\"{} - {}\" [{id:04X?}]",
                    device.vendor().name(),
                    device.name()
                )
            }
            None => {
                if let Some(vendor) = usb_ids::Vendor::from_id(vid) {
                    format!("\"{}\" - Unknown device. [{id:04X?}]", vendor.name())
                } else {
                    format!("Unknown vendor. [{id:04X?}]",)
                }
            }
        }
    } else {
        "No ID.".into()
    }
}

// Print a Strip or Ring event
fn format_touch_event(
    name: String,
    event: TouchStripEvent,
    fmt_pose: impl FnOnce(f32) -> String,
) -> (String, eframe::egui::Color32) {
    match event {
        TouchStripEvent::Pose(pose) => (format!("{name} pose {}", fmt_pose(pose)), colors::POSE),
        TouchStripEvent::Frame(time) => (
            if let Some(time) = time {
                format!("frame {time:?}")
            } else {
                "frame".into()
            },
            colors::TIME,
        ),
        TouchStripEvent::Source(src) => (format!("{name} interacted by {src:?}"), colors::SOURCE),
        TouchStripEvent::Up => (format!("{name} up"), colors::RELEASED),
    }
}

// Print out event in a nicer way than `Debug`, with some color too!
pub fn format_event(event: Event) -> (String, eframe::egui::Color32) {
    match event {
        Event::Tablet { tablet, event } => {
            let name = name_tablet(tablet);
            // default format
            match event {
                TabletEvent::Added => (format!("{} added", name), colors::ADDED),
                TabletEvent::Removed => (format!("{} removed", name), colors::REMOVED),
            }
        }
        Event::Pad { pad, event } => {
            let name = name_pad(pad);
            match event {
                PadEvent::Added => (format!("{name} added"), colors::ADDED),
                PadEvent::Removed => (format!("{name} removed"), colors::REMOVED),
                PadEvent::Enter { tablet } => {
                    let tablet = name_tablet(tablet);
                    (format!("{name} connected to {tablet}"), colors::ENTER)
                }
                PadEvent::Exit => (format!("{name} disconnected from tablet"), colors::EXIT),
                PadEvent::Button {
                    button_idx,
                    pressed,
                    group,
                } => {
                    // Find the index of this group. Much more friendly then a big opaque ID.
                    let group_id = group.map(octotablet::pad::Group::id);
                    let group_idx = group_id
                        .and_then(|id| pad.groups.iter().position(|group| group.id() == id));
                    (
                        format!(
                            "{name} button {button_idx} {} (owned by group {group_idx:?})",
                            if pressed { "pressed" } else { "released" }
                        ),
                        if pressed {
                            colors::PRESSED
                        } else {
                            colors::RELEASED
                        },
                    )
                }
                PadEvent::Group { group, event } => {
                    let name = format!("Group ({:08X?})", group.id());
                    match event {
                        PadGroupEvent::Mode(m) => {
                            (format!("{name} switched to mode {m}"), colors::MODE)
                        }
                        PadGroupEvent::Ring { ring, event } => {
                            let ring = ring.id();
                            format_touch_event(format!("Ring {ring:08X?}"), event, |pose| {
                                format!("{:.01}deg", pose.to_degrees())
                            })
                        }
                        PadGroupEvent::Strip { strip, event } => {
                            let strip = strip.id();
                            format_touch_event(format!("Strip {strip:08X?}"), event, |pose| {
                                format!("{:.01}%", pose * 100.0)
                            })
                        }
                    }
                }
            }
        }
        Event::Tool { tool, event } => {
            let name = name_tool(tool);
            match event {
                ToolEvent::Added => (format!("{name} added"), colors::ADDED),
                ToolEvent::Removed => (format!("{name} removed"), colors::REMOVED),
                ToolEvent::In { tablet } => {
                    let tablet = name_tablet(tablet);
                    (format!("{name} in over {tablet}"), colors::ENTER)
                }
                ToolEvent::Out => (format!("{name} out"), colors::EXIT),
                ToolEvent::Down => (format!("{name} down"), colors::PRESSED),
                ToolEvent::Up => (format!("{name} up"), colors::RELEASED),
                ToolEvent::Pose(pose) => (format!("{name} {pose:#?}"), colors::POSE),
                ToolEvent::Button { button_id, pressed } => (
                    format!(
                        "{name} button {button_id:08X?} {}",
                        if pressed { "pressed" } else { "released" }
                    ),
                    if pressed {
                        colors::PRESSED
                    } else {
                        colors::RELEASED
                    },
                ),
                ToolEvent::Frame(time) => (
                    if let Some(time) = time {
                        format!("Frame {time:?}")
                    } else {
                        "Frame".into()
                    },
                    colors::TIME,
                ),
            }
        }
    }
}
