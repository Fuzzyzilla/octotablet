//! `'static` versions of the events - the form in which they're stored when awaiting a pump,
//! and are converted on-the-fly to the more ergonomic event types.

#[derive(Clone, Debug)]
pub enum ToolEvent<Id> {
    Added,
    Removed,
    In {
        tablet: Id,
    },
    Down,
    Button {
        button_id: crate::platform::ButtonID,
        pressed: bool,
    },
    // This variant is many times the size of all the others resulting in huge inefficiency.
    // If memory usage/throughput becomes appreciably bad, this is a good place to start.
    Pose(super::Pose),
    Frame(Option<super::FrameTimestamp>),
    Up,
    Out,
}
impl<Id> ToolEvent<Id> {
    // Can't impl `From`, due to conflict with `From<T> for T` :(
    pub fn id_into<Into: From<Id>>(self) -> ToolEvent<Into> {
        match self {
            Self::Added => ToolEvent::Added,
            Self::Removed => ToolEvent::Removed,
            Self::In { tablet } => ToolEvent::In {
                tablet: Into::from(tablet),
            },
            Self::Down => ToolEvent::Down,
            Self::Button { button_id, pressed } => ToolEvent::Button { button_id, pressed },
            Self::Pose(v) => ToolEvent::Pose(v),
            Self::Frame(v) => ToolEvent::Frame(v),
            Self::Up => ToolEvent::Up,
            Self::Out => ToolEvent::Out,
        }
    }
}
#[derive(Clone, Debug)]
pub enum TabletEvent {
    Added,
    Removed,
}
#[derive(Clone, Debug)]
pub enum PadEvent<Id> {
    Added,
    Removed,
    Group { group: Id, event: PadGroupEvent<Id> },
    Button { button_idx: u32, pressed: bool },
    Enter { tablet: Id },
    Exit,
}
impl<Id> PadEvent<Id> {
    // Can't impl `From`, due to conflict with `From<T> for T` :(
    pub fn id_into<Into: From<Id>>(self) -> PadEvent<Into> {
        match self {
            Self::Added => PadEvent::Added,
            Self::Removed => PadEvent::Removed,
            Self::Group { group, event } => PadEvent::Group {
                group: Into::from(group),
                event: event.id_into(),
            },
            Self::Button {
                button_idx,
                pressed,
            } => PadEvent::Button {
                button_idx,
                pressed,
            },
            Self::Enter { tablet } => PadEvent::Enter {
                tablet: Into::from(tablet),
            },
            Self::Exit => PadEvent::Exit,
        }
    }
}
#[derive(Clone, Debug)]
pub enum PadGroupEvent<Id> {
    Ring {
        ring: Id,
        event: super::TouchStripEvent,
    },
    Strip {
        strip: Id,
        event: super::TouchStripEvent,
    },
    Mode(u32),
}
impl<Id> PadGroupEvent<Id> {
    // Can't impl `From`, due to conflict with `From<T> for T` :(
    pub fn id_into<Into: From<Id>>(self) -> PadGroupEvent<Into> {
        match self {
            Self::Ring { ring, event } => PadGroupEvent::Ring {
                ring: Into::from(ring),
                event,
            },
            Self::Strip { strip, event } => PadGroupEvent::Strip {
                strip: Into::from(strip),
                event,
            },
            Self::Mode(mode) => PadGroupEvent::Mode(mode),
        }
    }
}
#[derive(Clone, Debug)]
pub enum Event<Id> {
    Tool { tool: Id, event: ToolEvent<Id> },
    Tablet { tablet: Id, event: TabletEvent },
    Pad { pad: Id, event: PadEvent<Id> },
}
impl<Id> Event<Id> {
    // Can't impl `From`, due to conflict with `From<T> for T` :(
    pub fn id_into<Into: From<Id>>(self) -> Event<Into> {
        match self {
            Self::Tool { tool, event } => Event::Tool {
                tool: Into::from(tool),
                event: event.id_into::<Into>(),
            },
            Self::Tablet { tablet, event } => Event::Tablet {
                tablet: Into::from(tablet),
                event,
            },
            Self::Pad { pad, event } => Event::Pad {
                pad: Into::from(pad),
                event: event.id_into::<Into>(),
            },
        }
    }
}
