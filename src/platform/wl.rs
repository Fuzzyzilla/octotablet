//! Implementation details for Wayland's `tablet_unstable_v2` protocol.
//!
//! Within this module, it is sound to assume `cfg(wl_tablet) == true`
//! (compiling for a wayland target + has deps, or is building docs).
use crate::{
    events::raw as raw_events,
    pad::{Group, Ring, Strip, TouchSource},
};
pub type ID = wayland_backend::client::ObjectId;
use wayland_client::{
    protocol::{wl_registry, wl_seat},
    Connection, Dispatch, Proxy, QueueHandle,
};
use wayland_protocols::wp::tablet::zv2::client as wl_tablet;

use crate::{
    events::{FrameTimestamp, NicheF32, Pose},
    pad::Pad,
    tablet::{Tablet, UsbId},
    tool::{AvailableAxes, AxisInfo, Tool},
};

use super::InternalID;
pub struct Manager {
    _display: wayland_client::protocol::wl_display::WlDisplay,
    _conn: wayland_client::Connection,
    queue: wayland_client::EventQueue<TabletState>,
    _qh: wayland_client::QueueHandle<TabletState>,
    state: TabletState,
}
impl Manager {
    /// Creates a tablet manager with from the given pointer to `wl_display`.
    /// # Safety
    /// The given display pointer must be valid as long as the returned `Manager` is alive. The [`Backing`] parameter
    /// is kept alive with the returned Manager, which can be used to uphold this requirement.
    pub(crate) unsafe fn build_wayland_display(wl_display: *mut ()) -> Manager {
        // Safety - deferred to this fn's contract
        let backend =
            unsafe { wayland_backend::client::Backend::from_foreign_display(wl_display.cast()) };
        let conn = wayland_client::Connection::from_backend(backend);
        let display = conn.display();
        let queue = conn.new_event_queue();
        let qh = queue.handle();
        // Allow the manager impl to sift through and capture extention handles
        display.get_registry(&qh, ());
        Manager {
            _display: display,
            _conn: conn,
            queue,
            _qh: qh,
            state: TabletState::default(),
        }
    }
}
impl super::PlatformImpl for Manager {
    #[allow(clippy::missing_errors_doc)]
    fn pump(&mut self) -> Result<(), crate::PumpError> {
        self.state.cleanup_start();
        self.queue.dispatch_pending(&mut self.state)?;
        Ok(())
    }
    #[must_use]
    fn timestamp_granularity(&self) -> Option<std::time::Duration> {
        // Wayland always reports, and with millisecond granularity.
        Some(std::time::Duration::from_millis(1))
    }
    #[must_use]
    fn pads(&self) -> &[crate::pad::Pad] {
        &self.state.pads
    }
    #[must_use]
    fn tools(&self) -> &[crate::tool::Tool] {
        &self.state.tools
    }
    #[must_use]
    fn tablets(&self) -> &[crate::tablet::Tablet] {
        &self.state.tablets
    }
    fn raw_events(&self) -> super::RawEventsIter<'_> {
        super::RawEventsIter::Wayland(self.state.events.iter())
    }
    #[must_use]
    fn make_summary(&self) -> crate::events::summary::Summary {
        let try_summarize = || -> Option<crate::events::summary::Summary> {
            let sum = self.state.summary.as_ref()?;

            let tablet = self
                .tablets()
                .iter()
                .find(|tab| tab.internal_id.unwrap_wl() == &sum.tablet_id)?;
            let tool = self
                .tools()
                .iter()
                .find(|tab| tab.internal_id.unwrap_wl() == &sum.tool_id)?;
            Some(crate::events::summary::Summary {
                tool: crate::events::summary::ToolState::In(crate::events::summary::InState {
                    tablet,
                    tool,
                    pose: sum.pose,
                    down: sum.down,
                    pressed_buttons: &sum.buttons,
                    timestamp: Some(sum.time),
                }),
                pads: &[],
            })
        };

        // try block pls..
        try_summarize().unwrap_or(crate::events::summary::Summary {
            tool: crate::events::summary::ToolState::Out,
            pads: &[],
        })
    }
}

pub trait HasWlId: Sized {
    type DoneError;
    fn new_default(id: ID) -> Self;
    fn id(&self) -> &ID;
    /// Sent when constructors are done. Use this to
    /// make everything internally consistent.
    fn done(self) -> Result<Self, Self::DoneError>;
}
impl HasWlId for Tool {
    type DoneError = std::convert::Infallible;
    fn done(self) -> Result<Self, Self::DoneError> {
        Ok(self)
    }
    fn new_default(id: ID) -> Self {
        Tool {
            internal_id: id.into(),
            hardware_id: None,
            wacom_id: None,
            available_axes: AvailableAxes::empty(),
            tool_type: None,
            // Unfortunately, Wayland doesn't enumerate axis precision info. :<
            axis_info: Default::default(),
            position_info: AxisInfo::default(),
            distance_unit: crate::tool::DistanceUnit::Unitless,
        }
    }
    fn id(&self) -> &ID {
        // Unwrap OK - We only ever create `Wayland` instances, and it's not possible
        // for an externally created instance to get in here.
        self.internal_id.unwrap_wl()
    }
}
impl HasWlId for Tablet {
    type DoneError = std::convert::Infallible;
    fn done(self) -> Result<Self, Self::DoneError> {
        Ok(self)
    }
    fn new_default(id: ID) -> Self {
        Tablet {
            internal_id: id.into(),
            name: String::new(),
            usb_id: None,
        }
    }
    fn id(&self) -> &ID {
        // Unwrap OK - We only ever create `Wayland` instances, and it's not possible
        // for an externally created instance to get in here.
        self.internal_id.unwrap_wl()
    }
}
impl HasWlId for Pad {
    type DoneError = ();
    fn done(self) -> Result<Self, Self::DoneError> {
        if self.groups.is_empty() {
            Err(())
        } else {
            Ok(self)
        }
    }
    fn new_default(id: ID) -> Self {
        Pad {
            internal_id: id.into(),
            // 0 is the default and what the protocol specifies should be used if
            // the constructor for this value is never sent.
            total_buttons: 0,
            groups: Vec::new(),
        }
    }
    fn id(&self) -> &ID {
        // Unwrap OK - We only ever create `Wayland` instances, and it's not possible
        // for an externally created instance to get in here.
        self.internal_id.unwrap_wl()
    }
}
impl HasWlId for Group {
    type DoneError = std::convert::Infallible;
    fn done(self) -> Result<Self, Self::DoneError> {
        Ok(self)
    }
    fn new_default(id: ID) -> Self {
        Group {
            internal_id: InternalID::Wayland(id),
            buttons: Vec::new(),
            rings: Vec::new(),
            strips: Vec::new(),
            feedback: None,
            mode_count: None,
        }
    }
    fn id(&self) -> &ID {
        // Unwrap OK - We only ever create `Wayland` instances, and it's not possible
        // for an externally created instance to get in here.
        self.internal_id.unwrap_wl()
    }
}
impl HasWlId for Ring {
    type DoneError = std::convert::Infallible;
    fn done(self) -> Result<Self, Self::DoneError> {
        Ok(self)
    }
    fn new_default(id: ID) -> Self {
        Ring {
            internal_id: id.into(),
            granularity: None,
        }
    }
    fn id(&self) -> &ID {
        // Unwrap OK - We only ever create `Wayland` instances, and it's not possible
        // for an externally created instance to get in here.
        self.internal_id.unwrap_wl()
    }
}
impl HasWlId for Strip {
    type DoneError = std::convert::Infallible;
    fn done(self) -> Result<Self, Self::DoneError> {
        Ok(self)
    }
    fn new_default(id: ID) -> Self {
        Strip {
            internal_id: id.into(),
            granularity: None,
        }
    }
    fn id(&self) -> &ID {
        // Unwrap OK - We only ever create `Wayland` instances, and it's not possible
        // for an externally created instance to get in here.
        self.internal_id.unwrap_wl()
    }
}

#[derive(Clone)]
struct RawSummary {
    tool_id: ID,
    tablet_id: ID,
    down: bool,
    pose: Pose,
    // Names of the currently held buttons.
    buttons: smallvec::SmallVec<[u32; 4]>,
    time: FrameTimestamp,
}

/// Tracks objects which are in the process of being created by a
/// data burst followed by "done".
pub(crate) struct PartialVec<T> {
    pub(crate) constructing: Vec<T>,
}
impl<T> Default for PartialVec<T> {
    fn default() -> Self {
        Self {
            constructing: vec![],
        }
    }
}
impl<T: HasWlId> PartialVec<T> {
    fn get_or_insert_ctor(&mut self, id: ID) -> &mut T {
        let index = if let Some(found) = self.constructing.iter().position(|obj| obj.id() == &id) {
            found
        } else {
            self.constructing.push(T::new_default(id));
            self.constructing.len() - 1
        };

        &mut self.constructing[index]
    }
    /// Try to finish and return the object in progress. None if no such object, Some(Err) if the object
    /// determined that it was not internally-consistent.
    fn done(&mut self, id: &ID) -> Option<Result<T, T::DoneError>> {
        if let Some(finished_idx) = self.constructing.iter().position(|obj| obj.id() == id) {
            Some(self.constructing.remove(finished_idx).done())
        } else {
            None
        }
    }
    /// Drop in-progress items.
    fn destroy(&mut self, id: &ID) {
        self.constructing.retain(|obj| obj.id() != id);
    }
}

/// only one or none of these events may happen per tool per frame.
#[derive(PartialEq, Eq)]
enum FrameState {
    In(ID),
    Out,
    Down,
    Up,
}

/// Keeps track of partial frame data that's in the processes of being assembled from messages.
struct FrameInProgress {
    tool: ID,
    state_transition: Option<FrameState>,
    position: Option<[f32; 2]>,
    distance: Option<f32>,
    pressure: Option<f32>,
    tilt: Option<[f32; 2]>,
    roll: Option<f32>,
    wheel: Option<(f32, i32)>,
    slider: Option<f32>,
    // Stream of button events that happened during this frame.
    // By the nature of frames, these are considered to have happened at the same time, but order is still preserved.
    buttons: smallvec::SmallVec<[(u32, bool); 1]>,
}

enum ConstructID {
    Tablet(ID),
    Pad(ID),
    Tool(ID),
}

#[derive(Default)]
struct TabletState {
    // Internal goobers
    seat: Option<wl_seat::WlSeat>,
    manager: Option<wl_tablet::zwp_tablet_manager_v2::ZwpTabletManagerV2>,
    tablet_seat: Option<wl_tablet::zwp_tablet_seat_v2::ZwpTabletSeatV2>,
    // Space for in-progress constructor executions.
    partial_tablets: PartialVec<Tablet>,
    partial_tools: PartialVec<Tool>,
    partial_pads: PartialVec<Pad>,
    partial_groups: PartialVec<Group>,
    // Completed constructions
    tablets: Vec<Tablet>,
    tools: Vec<Tool>,
    pads: Vec<Pad>,
    // Things to destroy:
    destroy_next_frame: Vec<ConstructID>,
    // Associations for which pad group each ring and strip are connected
    // `{ring or strip} -> group`
    ring_associations: std::collections::HashMap<ID, ID>,
    strip_associations: std::collections::HashMap<ID, ID>,
    // Associations for which pad each group is connected
    // `group -> pad`
    group_associations: std::collections::HashMap<ID, ID>,
    // Partial and complete event/summary tracking.
    summary: Option<RawSummary>,
    frames_in_progress: Vec<FrameInProgress>,
    events: Vec<crate::events::raw::Event<ID>>,
}
impl TabletState {
    fn destroy_tool(&mut self, tool: ID) {
        self.partial_tools.destroy(&tool);
        // Defer destruction, that way `Removed` events can still refer by reference.
        self.destroy_next_frame.push(ConstructID::Tool(tool));
    }
    fn destroy_tablet(&mut self, tablet: ID) {
        self.partial_tablets.destroy(&tablet);
        // Defer destruction, that way `Removed` events can still refer by reference.
        self.destroy_next_frame.push(ConstructID::Tablet(tablet));
    }
    fn destroy_pad(&mut self, pad: ID) {
        self.partial_pads.destroy(&pad);
        // Defer destruction, that way `Removed` events can still refer by reference.
        self.destroy_next_frame.push(ConstructID::Pad(pad));
    }
    // Start of a pump, clean up the leftover tasks from last pump:
    fn cleanup_start(&mut self) {
        self.events.clear();
        for destroy in self.destroy_next_frame.drain(..) {
            match destroy {
                ConstructID::Pad(id) => self.pads.retain(|p| HasWlId::id(p) != &id),
                ConstructID::Tablet(id) => self.tablets.retain(|t| HasWlId::id(t) != &id),
                ConstructID::Tool(id) => {
                    self.tools.retain(|t| HasWlId::id(t) != &id);
                    self.frames_in_progress.retain(|f| f.tool != id);
                }
            }
        }
    }
    // Create or get the partially built frame.
    fn frame_in_progress(&mut self, tool: ID) -> &mut FrameInProgress {
        let pos = self
            .frames_in_progress
            .iter()
            .position(|frame| frame.tool == tool);
        if let Some(pos) = pos {
            &mut self.frames_in_progress[pos]
        } else {
            self.frames_in_progress.push(FrameInProgress {
                tool,
                state_transition: None,
                position: None,
                distance: None,
                pressure: None,
                tilt: None,
                roll: None,
                wheel: None,
                slider: None,
                buttons: smallvec::SmallVec::new(),
            });
            self.frames_in_progress.last_mut().unwrap()
        }
    }
    fn frame(&mut self, tool: &ID, millis: u32) {
        // Emit the frame. Notably, we leave the frame intact - only changed values are reported by the server,
        // so this allows previous values to be inherited.
        let clear = if let Some(frame) = self
            .frames_in_progress
            .iter_mut()
            .find(|frame| &frame.tool == tool)
        {
            // Provide strong ordering of events within a frame in an intuitive way, despite the fact that
            // they're to be interpreted as all having happened similtaneously. Explicitly not an API-level guarantee, should it be?

            // emit ins and downs first...
            match frame.state_transition {
                Some(FrameState::In(ref tablet)) => self.events.push(raw_events::Event::Tool {
                    tool: tool.clone(),
                    event: raw_events::ToolEvent::In {
                        tablet: tablet.clone(),
                    },
                }),
                Some(FrameState::Down) => self.events.push(raw_events::Event::Tool {
                    tool: tool.clone(),
                    event: raw_events::ToolEvent::Down,
                }),
                _ => (),
            }
            // Emit pose...
            // Position is the only required axis.
            // We explicity do *not* check that the reported axes line up with the capabilities of the tool.
            // The reported capabilities often lie - we leave this to the user to handle, by just reporting every
            // axis it gave data for with no regard for capabilities.
            if let Some(position) = frame.position.filter(|[x, y]| !x.is_nan() && !y.is_nan()) {
                // Filter to prevent NaN's. This is not currently an invariant we guarantee since I can't figure out how
                // to ergonomically express it at the type level, but the legwork is already done:
                let pose = Pose {
                    position,
                    // Try to make the Option into Niche'd option. If NaN, fail back to None.
                    distance: frame.distance.try_into().unwrap_or(NicheF32::NONE),
                    pressure: frame.pressure.try_into().unwrap_or(NicheF32::NONE),
                    roll: frame.roll.try_into().unwrap_or(NicheF32::NONE),
                    slider: frame.slider.try_into().unwrap_or(NicheF32::NONE),
                    tilt: frame.tilt.filter(|[x, y]| !x.is_nan() && !y.is_nan()),
                    wheel: frame.wheel.filter(|(delta, _)| !delta.is_nan()),
                };
                // Make double extra sure!
                pose.debug_assert_not_nan();

                self.events.push(raw_events::Event::Tool {
                    tool: tool.clone(),
                    event: raw_events::ToolEvent::Pose(pose),
                });
            }
            // Emit buttons...
            for &(button_id, pressed) in &frame.buttons {
                self.events.push(raw_events::Event::Tool {
                    tool: tool.clone(),
                    event: raw_events::ToolEvent::Button { button_id, pressed },
                });
            }
            // emit ups and outs last... Return true from Out to mark the frame for clearing.
            let clear = match frame.state_transition {
                Some(FrameState::Up) => {
                    self.events.push(raw_events::Event::Tool {
                        tool: tool.clone(),
                        event: raw_events::ToolEvent::Up,
                    });
                    // We're still In - leave the frame intact.
                    false
                }
                Some(FrameState::Out) => {
                    self.events.push(raw_events::Event::Tool {
                        tool: tool.clone(),
                        event: raw_events::ToolEvent::Out,
                    });
                    // Out - destroy the partial frame afterwards.
                    true
                }
                _ => false,
            };
            // Frame finished. Remove all one-shot components.
            frame.state_transition = None;
            frame.buttons.clear();
            clear
        } else {
            false
        };
        // Emit frame. This may be an empty frame if above was None, that's alright!
        self.events.push(raw_events::Event::Tool {
            tool: tool.clone(),
            event: raw_events::ToolEvent::Frame(Some(FrameTimestamp(
                std::time::Duration::from_millis(u64::from(millis)),
            ))),
        });

        if clear {
            // Marked for deletion.
            self.frames_in_progress.retain(|frame| &frame.tool != tool);
        }
    }
    fn try_acquire_tablet_seat(&mut self, qh: &QueueHandle<Self>) {
        if self.tablet_seat.is_some() {
            return;
        }
        if let Some((seat, tablet)) = self.seat.as_ref().zip(self.manager.as_ref()) {
            self.tablet_seat = Some(tablet.get_tablet_seat(seat, qh, ()));
        }
    }
}

impl Dispatch<wl_registry::WlRegistry, ()> for TabletState {
    fn event(
        this: &mut Self,
        registry: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        (): &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wl_registry::Event::Global {
            name,
            interface,
            version,
        } = event
        {
            match &interface[..] {
                "wl_seat" => {
                    this.seat = Some(registry.bind(name, version, qh, ()));
                    // Need a seat and a tablet manager to bind tablet seat.
                    this.try_acquire_tablet_seat(qh);
                }
                "zwp_tablet_manager_v2" => {
                    this.manager = Some(registry.bind(name, version, qh, ()));
                    // Need a seat and a tablet manager to bind tablet seat.
                    this.try_acquire_tablet_seat(qh);
                }
                _ => (),
            }
        }
    }
}
impl Dispatch<wl_seat::WlSeat, ()> for TabletState {
    fn event(
        _: &mut Self,
        _: &wl_seat::WlSeat,
        _: wl_seat::Event,
        (): &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        // We need to acquire the seat for binding a tablet to it, but we do not
        // care what the seat says D:
    }
}
impl Dispatch<wl_tablet::zwp_tablet_manager_v2::ZwpTabletManagerV2, ()> for TabletState {
    fn event(
        _: &mut Self,
        _: &wl_tablet::zwp_tablet_manager_v2::ZwpTabletManagerV2,
        _: wl_tablet::zwp_tablet_manager_v2::Event,
        (): &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        // NEVER ENTERS, DON'T PUT STUFF HERE
        unreachable!()
    }
}
impl Dispatch<wl_tablet::zwp_tablet_seat_v2::ZwpTabletSeatV2, ()> for TabletState {
    fn event(
        _: &mut Self,
        _seat: &wl_tablet::zwp_tablet_seat_v2::ZwpTabletSeatV2,
        event: wl_tablet::zwp_tablet_seat_v2::Event,
        (): &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        use wl_tablet::zwp_tablet_seat_v2::Event;
        // We handle these each "lazily" - the first init message triggers device addition logic.
        #[allow(clippy::match_same_arms)]
        match event {
            Event::PadAdded { .. } => (),
            Event::TabletAdded { .. } => (),
            Event::ToolAdded { .. } => (),
            // ne
            _ => (),
        }
    }
    wayland_client::event_created_child!(
        TabletState,
        wl_tablet::zwp_tablet_seat_v2::ZwpTabletSeatV2,
        [
            wl_tablet::zwp_tablet_seat_v2::EVT_PAD_ADDED_OPCODE => (wl_tablet::zwp_tablet_pad_v2::ZwpTabletPadV2, ()),
            wl_tablet::zwp_tablet_seat_v2::EVT_TABLET_ADDED_OPCODE => (wl_tablet::zwp_tablet_v2::ZwpTabletV2, ()),
            wl_tablet::zwp_tablet_seat_v2::EVT_TOOL_ADDED_OPCODE => (wl_tablet::zwp_tablet_tool_v2::ZwpTabletToolV2, ()),
        ]
    );
}
impl Dispatch<wl_tablet::zwp_tablet_v2::ZwpTabletV2, ()> for TabletState {
    fn event(
        this: &mut Self,
        tablet: &wl_tablet::zwp_tablet_v2::ZwpTabletV2,
        event: wl_tablet::zwp_tablet_v2::Event,
        (): &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        use wl_tablet::zwp_tablet_v2::Event;
        #[allow(clippy::match_same_arms)]
        match event {
            // ======= Constructor databurst =========
            Event::Done => {
                this.events.push(raw_events::Event::Tablet {
                    tablet: tablet.id(),
                    event: raw_events::TabletEvent::Added,
                });
                if let Some(Ok(tablet)) = this.partial_tablets.done(&tablet.id()) {
                    this.tablets.push(tablet);
                }
            }
            Event::Id { vid, pid } => {
                // Convert to u16s (have been crammed into u32s...) and set, if any.
                this.partial_tablets.get_or_insert_ctor(tablet.id()).usb_id = u16::try_from(vid)
                    .ok()
                    .zip(u16::try_from(pid).ok())
                    .map(|(vid, pid)| UsbId { vid, pid });
            }
            Event::Name { name } => {
                this.partial_tablets.get_or_insert_ctor(tablet.id()).name = name;
            }
            Event::Path { .. } => (),
            Event::Removed => {
                this.destroy_tablet(tablet.id());
                this.events.push(raw_events::Event::Tablet {
                    tablet: tablet.id(),
                    event: raw_events::TabletEvent::Removed,
                });
            }
            // ne
            _ => (),
        }
    }
}
impl Dispatch<wl_tablet::zwp_tablet_tool_v2::ZwpTabletToolV2, ()> for TabletState {
    #[allow(clippy::too_many_lines)]
    fn event(
        this: &mut Self,
        tool: &wl_tablet::zwp_tablet_tool_v2::ZwpTabletToolV2,
        event: wl_tablet::zwp_tablet_tool_v2::Event,
        (): &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        use wl_tablet::zwp_tablet_tool_v2::Event;
        #[allow(clippy::match_same_arms)]
        match event {
            // ======= Constructor databurst =======
            Event::Capability {
                capability: wayland_client::WEnum::Value(capability),
            } => {
                use wl_tablet::zwp_tablet_tool_v2::Capability;
                let ctor = this.partial_tools.get_or_insert_ctor(tool.id());
                match capability {
                    Capability::Distance => ctor.available_axes.insert(AvailableAxes::DISTANCE),
                    Capability::Pressure => ctor.available_axes.insert(AvailableAxes::PRESSURE),
                    Capability::Rotation => ctor.available_axes.insert(AvailableAxes::ROLL),
                    Capability::Slider => ctor.available_axes.insert(AvailableAxes::SLIDER),
                    Capability::Tilt => ctor.available_axes.insert(AvailableAxes::TILT),
                    Capability::Wheel => ctor.available_axes.insert(AvailableAxes::WHEEL),
                    // ne
                    _ => (),
                }
            }
            Event::HardwareIdWacom {
                hardware_id_hi,
                hardware_id_lo,
            } => {
                let ctor = this.partial_tools.get_or_insert_ctor(tool.id());
                ctor.wacom_id = Some(u64::from(hardware_id_hi) << 32 | u64::from(hardware_id_lo));
            }
            Event::HardwareSerial {
                hardware_serial_hi,
                hardware_serial_lo,
            } => {
                let ctor = this.partial_tools.get_or_insert_ctor(tool.id());
                ctor.hardware_id =
                    Some(u64::from(hardware_serial_hi) << 32 | u64::from(hardware_serial_lo));
            }
            Event::Type {
                tool_type: wayland_client::WEnum::Value(tool_type),
            } => {
                use crate::tool::Type;
                use wl_tablet::zwp_tablet_tool_v2::Type as WlType;
                let ctor = this.partial_tools.get_or_insert_ctor(tool.id());
                match tool_type {
                    WlType::Airbrush => ctor.tool_type = Some(Type::Airbrush),
                    WlType::Brush => ctor.tool_type = Some(Type::Brush),
                    WlType::Eraser => ctor.tool_type = Some(Type::Eraser),
                    WlType::Finger => ctor.tool_type = Some(Type::Finger),
                    WlType::Lens => ctor.tool_type = Some(Type::Lens),
                    WlType::Mouse => ctor.tool_type = Some(Type::Mouse),
                    WlType::Pen => ctor.tool_type = Some(Type::Pen),
                    WlType::Pencil => ctor.tool_type = Some(Type::Pencil),
                    // ne
                    _ => (),
                }
            }
            Event::Done => {
                this.events.push(raw_events::Event::Tool {
                    tool: tool.id(),
                    event: raw_events::ToolEvent::Added,
                });
                if let Some(Ok(tool)) = this.partial_tools.done(&tool.id()) {
                    this.tools.push(tool);
                }
            }
            Event::Removed => {
                this.destroy_tool(tool.id());
                this.events.push(raw_events::Event::Tool {
                    tool: tool.id(),
                    event: raw_events::ToolEvent::Removed,
                });
            }
            // ======== Interaction data =========
            Event::ProximityIn { tablet, .. } => {
                this.frame_in_progress(tool.id()).state_transition =
                    Some(FrameState::In(tablet.id()));
                this.summary = Some(RawSummary {
                    tablet_id: tablet.id(),
                    tool_id: tool.id(),
                    down: false,
                    buttons: smallvec::smallvec![],
                    pose: Pose::default(),
                    time: FrameTimestamp::epoch(),
                });
            }
            Event::ProximityOut { .. } => {
                this.frame_in_progress(tool.id()).state_transition = Some(FrameState::Out);
                if this
                    .summary
                    .as_ref()
                    .is_some_and(|sum| sum.tool_id == tool.id())
                {
                    this.summary = None;
                }
            }
            Event::Down { .. } => {
                this.frame_in_progress(tool.id()).state_transition = Some(FrameState::Down);
                if let Some(summary) = &mut this.summary {
                    if summary.tool_id == tool.id() {
                        summary.down = true;
                    }
                }
            }
            Event::Up { .. } => {
                this.frame_in_progress(tool.id()).state_transition = Some(FrameState::Up);
                if let Some(summary) = &mut this.summary {
                    if summary.tool_id == tool.id() {
                        summary.down = false;
                    }
                }
            }
            #[allow(clippy::cast_possible_truncation)]
            Event::Motion { x, y } => {
                let x = x as f32;
                let y = y as f32;
                this.frame_in_progress(tool.id()).position = Some([x, y]);
                if let Some(summary) = &mut this.summary {
                    // shhh...
                    #[allow(clippy::cast_possible_truncation)]
                    if summary.tool_id == tool.id() {
                        summary.pose.position = [x, y];
                    }
                }
            }
            #[allow(clippy::cast_possible_truncation)]
            Event::Tilt { tilt_x, tilt_y } => {
                let tilt_x = (tilt_x as f32).to_radians();
                let tilt_y = (tilt_y as f32).to_radians();
                this.frame_in_progress(tool.id()).tilt = Some([tilt_x, tilt_y]);
                if let Some(summary) = &mut this.summary {
                    // shhh...
                    #[allow(clippy::cast_possible_truncation)]
                    if summary.tool_id == tool.id() {
                        summary.pose.tilt = Some([tilt_x, tilt_y]);
                    }
                }
            }
            Event::Pressure { pressure } => {
                // Saturating-as (guaranteed by the protocol spec to be 0..=65535)
                let pressure = u16::try_from(pressure).unwrap_or(65535);
                let pressure = f32::from(pressure) / 65535.0;
                this.frame_in_progress(tool.id()).pressure = Some(pressure);
                if let Some(summary) = &mut this.summary {
                    #[allow(clippy::cast_precision_loss)]
                    if summary.tool_id == tool.id() {
                        summary.pose.pressure = NicheF32::new_some(pressure).unwrap();
                    }
                }
            }
            Event::Distance { distance } => {
                // Saturating-as (guaranteed by the protocol spec to be 0..=65535)
                let distance = u16::try_from(distance).unwrap_or(65535);
                let distance = f32::from(distance) / 65535.0;
                this.frame_in_progress(tool.id()).distance = Some(distance);
                if let Some(summary) = &mut this.summary {
                    #[allow(clippy::cast_precision_loss)]
                    if summary.tool_id == tool.id() {
                        summary.pose.distance = NicheF32::new_some(distance).unwrap();
                    }
                }
            }
            #[allow(clippy::cast_possible_truncation)]
            Event::Rotation { degrees } => {
                let radians = (degrees as f32).to_radians();
                this.frame_in_progress(tool.id()).roll = Some(radians);
                if let Some(summary) = &mut this.summary {
                    #[allow(clippy::cast_possible_truncation)]
                    if summary.tool_id == tool.id() {
                        summary.pose.roll = NicheF32::new_some(radians).unwrap();
                    }
                }
            }
            Event::Slider { position } => {
                // Saturating-as (guaranteed by the protocol spec to be 0..=65535)
                let position = u16::try_from(position).unwrap_or(65535);
                let position = f32::from(position) / 65535.0;
                this.frame_in_progress(tool.id()).slider = Some(position);
                if let Some(summary) = &mut this.summary {
                    #[allow(clippy::cast_precision_loss)]
                    if summary.tool_id == tool.id() {
                        summary.pose.slider = NicheF32::new_some(position).unwrap();
                    }
                }
            }
            Event::Button { button, state, .. } => {
                let pressed = matches!(
                    state,
                    wayland_client::WEnum::Value(
                        wl_tablet::zwp_tablet_tool_v2::ButtonState::Pressed
                    )
                );
                this.frame_in_progress(tool.id())
                    .buttons
                    .push((button, pressed));
                if let Some(summary) = &mut this.summary {
                    if summary.tool_id == tool.id() {
                        if pressed {
                            // Add id if not already present
                            if !summary.buttons.contains(&button) {
                                summary.buttons.push(button);
                            }
                        } else {
                            // clear id from the set
                            summary.buttons.retain(|b| *b != button);
                        }
                    }
                }
            }
            Event::Frame { time } => {
                if let Some(summary) = &mut this.summary {
                    if summary.tool_id == tool.id() {
                        summary.time =
                            FrameTimestamp(std::time::Duration::from_millis(u64::from(time)));
                    }
                }
                this.frame(&tool.id(), time);
            }

            // ne
            _ => (),
        }
    }
}
impl Dispatch<wl_tablet::zwp_tablet_pad_v2::ZwpTabletPadV2, ()> for TabletState {
    fn event(
        this: &mut Self,
        pad: &wl_tablet::zwp_tablet_pad_v2::ZwpTabletPadV2,
        event: wl_tablet::zwp_tablet_pad_v2::Event,
        (): &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        use wl_tablet::zwp_tablet_pad_v2::Event;
        #[allow(clippy::match_same_arms)]
        match event {
            // ======= Constructor databurst =========
            Event::Group { pad_group } => {
                let ctor = this.partial_pads.get_or_insert_ctor(pad.id());
                ctor.groups.push(Group::new_default(pad_group.id()));
                // Remember that this group id is associated with this pad.
                this.group_associations.insert(pad_group.id(), pad.id());
            }
            Event::Path { .. } => (),
            Event::Buttons { buttons } => {
                let ctor = this.partial_pads.get_or_insert_ctor(pad.id());
                ctor.total_buttons = buttons;
            }
            Event::Done => {
                let pad_id = pad.id();
                if let Some(Ok(pad)) = this.partial_pads.done(&pad_id) {
                    this.pads.push(pad);
                    this.events.push(raw_events::Event::Pad {
                        pad: pad_id,
                        event: raw_events::PadEvent::Added,
                    });
                }
            }
            Event::Removed => {
                this.destroy_pad(pad.id());
                this.events.push(raw_events::Event::Pad {
                    pad: pad.id(),
                    event: raw_events::PadEvent::Removed,
                });
            }
            // ======== Interaction data =========
            Event::Button {
                button,
                state,
                time, //left to warn on purpose - use me!
            } => {
                let pressed = matches!(
                    state,
                    wayland_client::WEnum::Value(
                        wl_tablet::zwp_tablet_pad_v2::ButtonState::Pressed
                    )
                );
                this.events.push(raw_events::Event::Pad {
                    pad: pad.id(),
                    event: raw_events::PadEvent::Button {
                        button_idx: button,
                        pressed,
                    },
                });
            }
            Event::Enter { tablet, .. } => this.events.push(raw_events::Event::Pad {
                pad: pad.id(),
                event: raw_events::PadEvent::Enter {
                    tablet: tablet.id(),
                },
            }),
            Event::Leave { .. } => this.events.push(raw_events::Event::Pad {
                pad: pad.id(),
                event: raw_events::PadEvent::Exit,
            }),
            // ne
            _ => (),
        }
    }
    wayland_client::event_created_child!(
        TabletState,
        wl_tablet::zwp_tablet_pad_v2::ZwpTabletPadV2,
        [
            wl_tablet::zwp_tablet_pad_v2::EVT_GROUP_OPCODE => (wl_tablet::zwp_tablet_pad_group_v2::ZwpTabletPadGroupV2, ()),
        ]
    );
}
impl Dispatch<wl_tablet::zwp_tablet_pad_group_v2::ZwpTabletPadGroupV2, ()> for TabletState {
    fn event(
        this: &mut Self,
        group: &wl_tablet::zwp_tablet_pad_group_v2::ZwpTabletPadGroupV2,
        event: wl_tablet::zwp_tablet_pad_group_v2::Event,
        (): &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        // Everything (aside from the ctor databurst) needs this. Hoist it out for less code duplication...
        let pad_id = this.group_associations.get(&group.id()).cloned();
        #[allow(clippy::match_same_arms)]
        match event {
            // ======= Constructor databurst =========
            wl_tablet::zwp_tablet_pad_group_v2::Event::Buttons { buttons } => {
                // Buttons *seems* to be a byte array, where each chunk of 4 is a `u32` in native endian order,
                // listing the indices of the global pad buttons that are uniquely owned by this group.
                // This is all just my best guess! Ahh!
                // Truncates to four byte chunks. That seems like a server error if my interpretation of the arcane
                // values are correct.
                let to_u32 = buttons.chunks_exact(4).map(|bytes| {
                    let Ok(bytes): Result<[u8; 4], _> = bytes.try_into() else {
                        // Guaranteed by chunks exact but not shown at a type-level.
                        unreachable!()
                    };
                    u32::from_ne_bytes(bytes)
                });
                let ctor = this.partial_groups.get_or_insert_ctor(group.id());
                ctor.buttons.extend(to_u32);
                // Make some adjustments to be slightly more reasonable to use x3
                // These collections will be trivially tiny so this is fine to do.
                ctor.buttons.sort_unstable();
                ctor.buttons.dedup();
            }
            wl_tablet::zwp_tablet_pad_group_v2::Event::Modes { modes } => {
                // This event only sent when modes > 0.
                let ctor = this.partial_groups.get_or_insert_ctor(group.id());
                // Will always be Some(modes), but panicless in the case of a server impl bug.
                ctor.mode_count = std::num::NonZeroU32::new(modes);
            }
            wl_tablet::zwp_tablet_pad_group_v2::Event::Ring { ring } => {
                this.ring_associations.insert(ring.id(), group.id());
                let ctor = this.partial_groups.get_or_insert_ctor(group.id());
                ctor.rings.push(Ring {
                    granularity: None,
                    internal_id: ring.id().into(),
                });
            }
            wl_tablet::zwp_tablet_pad_group_v2::Event::Strip { strip } => {
                this.strip_associations.insert(strip.id(), group.id());
                let ctor = this.partial_groups.get_or_insert_ctor(group.id());
                ctor.strips.push(Strip {
                    granularity: None,
                    internal_id: strip.id().into(),
                });
            }
            wl_tablet::zwp_tablet_pad_group_v2::Event::Done => {
                // Finish the group and add it the associated pad.
                // *Confused screaming*
                let group_id = group.id();
                if let Some(Ok(group)) = this.partial_groups.done(&group_id) {
                    if let Some(pad_id) = pad_id {
                        // Pad may be finished already or still in construction.
                        let pad = if let Some(pad) =
                            this.pads.iter_mut().find(|p| HasWlId::id(*p) == &pad_id)
                        {
                            pad
                        } else {
                            this.partial_pads.get_or_insert_ctor(pad_id)
                        };
                        // Replace existing group of this id, or add new.
                        // This is all weird hacky ctor ordering nonsense...
                        if let Some(pos) =
                            pad.groups.iter().position(|g| HasWlId::id(g) == &group_id)
                        {
                            pad.groups[pos] = group;
                        } else {
                            pad.groups.push(group);
                        }
                    }
                }
            }
            // ======== Interaction data =========
            wl_tablet::zwp_tablet_pad_group_v2::Event::ModeSwitch {
                mode,
                time, //left to warn on purpose - use me!
                ..
            } => {
                let Some(pad) = pad_id else { return };
                this.events.push(raw_events::Event::Pad {
                    pad,
                    event: raw_events::PadEvent::Group {
                        group: group.id(),
                        event: raw_events::PadGroupEvent::Mode(mode),
                    },
                });
            }
            // ne
            _ => (),
        }
    }
    wayland_client::event_created_child!(
        TabletState,
        wl_tablet::zwp_tablet_pad_group_v2::ZwpTabletPadGroupV2,
        [
            wl_tablet::zwp_tablet_pad_group_v2::EVT_RING_OPCODE => (wl_tablet::zwp_tablet_pad_ring_v2::ZwpTabletPadRingV2, ()),
            wl_tablet::zwp_tablet_pad_group_v2::EVT_STRIP_OPCODE => (wl_tablet::zwp_tablet_pad_strip_v2::ZwpTabletPadStripV2, ()),
        ]
    );
}
impl Dispatch<wl_tablet::zwp_tablet_pad_ring_v2::ZwpTabletPadRingV2, ()> for TabletState {
    fn event(
        this: &mut Self,
        ring: &wl_tablet::zwp_tablet_pad_ring_v2::ZwpTabletPadRingV2,
        event: wl_tablet::zwp_tablet_pad_ring_v2::Event,
        (): &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let Some(group) = this.ring_associations.get(&ring.id()).cloned() else {
            return;
        };
        let Some(pad) = this.group_associations.get(&group).cloned() else {
            return;
        };
        #[allow(clippy::match_same_arms)]
        match event {
            #[allow(clippy::cast_possible_truncation)]
            wl_tablet::zwp_tablet_pad_ring_v2::Event::Angle { degrees } => {
                if degrees.is_nan() {
                    return;
                }
                let degrees = degrees as f32;
                let radians = degrees.to_radians();
                this.events.push(raw_events::Event::Pad {
                    pad,
                    event: raw_events::PadEvent::Group {
                        group,
                        event: raw_events::PadGroupEvent::Ring {
                            ring: ring.id(),
                            event: crate::events::TouchStripEvent::Pose(radians),
                        },
                    },
                });
            }
            wl_tablet::zwp_tablet_pad_ring_v2::Event::Frame { time } => {
                this.events.push(raw_events::Event::Pad {
                    pad,
                    event: raw_events::PadEvent::Group {
                        group,
                        event: raw_events::PadGroupEvent::Ring {
                            ring: ring.id(),
                            event: crate::events::TouchStripEvent::Frame(Some(FrameTimestamp(
                                std::time::Duration::from_millis(u64::from(time)),
                            ))),
                        },
                    },
                });
            }
            wl_tablet::zwp_tablet_pad_ring_v2::Event::Source { source } => {
                // Convert source, or skip if not known.
                let source = match source {
                    wayland_client::WEnum::Value(v) => {
                        match v {
                            wl_tablet::zwp_tablet_pad_ring_v2::Source::Finger => {
                                TouchSource::Finger
                            }
                            // ne
                            _ => return,
                        }
                    }
                    wayland_client::WEnum::Unknown(_) => return,
                };
                this.events.push(raw_events::Event::Pad {
                    pad,
                    event: raw_events::PadEvent::Group {
                        group,
                        event: raw_events::PadGroupEvent::Ring {
                            ring: ring.id(),
                            event: crate::events::TouchStripEvent::Source(source),
                        },
                    },
                });
            }
            wl_tablet::zwp_tablet_pad_ring_v2::Event::Stop => {
                this.events.push(raw_events::Event::Pad {
                    pad,
                    event: raw_events::PadEvent::Group {
                        group,
                        event: raw_events::PadGroupEvent::Ring {
                            ring: ring.id(),
                            event: crate::events::TouchStripEvent::Up,
                        },
                    },
                });
            }
            // ne
            _ => (),
        }
    }
}
impl Dispatch<wl_tablet::zwp_tablet_pad_strip_v2::ZwpTabletPadStripV2, ()> for TabletState {
    fn event(
        this: &mut Self,
        strip: &wl_tablet::zwp_tablet_pad_strip_v2::ZwpTabletPadStripV2,
        event: wl_tablet::zwp_tablet_pad_strip_v2::Event,
        (): &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        // BIIGGGG code duplication with Ring, i don't know how to fix that because this all comes from different modules and thus
        // is actually different types......
        let Some(group) = this.strip_associations.get(&strip.id()).cloned() else {
            return;
        };
        let Some(pad) = this.strip_associations.get(&group).cloned() else {
            return;
        };
        #[allow(clippy::match_same_arms)]
        match event {
            #[allow(clippy::cast_possible_truncation)]
            wl_tablet::zwp_tablet_pad_strip_v2::Event::Position { position } => {
                // Saturating-as (guaranteed by the protocol spec to be 0..=65535)
                let position = u16::try_from(position).unwrap_or(65535);
                let position = f32::from(position) / 65535.0;
                this.events.push(raw_events::Event::Pad {
                    pad,
                    event: raw_events::PadEvent::Group {
                        group,
                        event: raw_events::PadGroupEvent::Strip {
                            strip: strip.id(),
                            event: crate::events::TouchStripEvent::Pose(position),
                        },
                    },
                });
            }
            wl_tablet::zwp_tablet_pad_strip_v2::Event::Frame { time } => {
                this.events.push(raw_events::Event::Pad {
                    pad,
                    event: raw_events::PadEvent::Group {
                        group,
                        event: raw_events::PadGroupEvent::Strip {
                            strip: strip.id(),
                            event: crate::events::TouchStripEvent::Frame(Some(FrameTimestamp(
                                std::time::Duration::from_millis(u64::from(time)),
                            ))),
                        },
                    },
                });
            }
            wl_tablet::zwp_tablet_pad_strip_v2::Event::Source { source } => {
                // Convert source, or skip if not known.
                let source = match source {
                    wayland_client::WEnum::Value(v) => {
                        match v {
                            wl_tablet::zwp_tablet_pad_strip_v2::Source::Finger => {
                                TouchSource::Finger
                            }
                            // ne
                            _ => return,
                        }
                    }
                    wayland_client::WEnum::Unknown(_) => return,
                };
                this.events.push(raw_events::Event::Pad {
                    pad,
                    event: raw_events::PadEvent::Group {
                        group,
                        event: raw_events::PadGroupEvent::Strip {
                            strip: strip.id(),
                            event: crate::events::TouchStripEvent::Source(source),
                        },
                    },
                });
            }
            wl_tablet::zwp_tablet_pad_strip_v2::Event::Stop => {
                this.events.push(raw_events::Event::Pad {
                    pad,
                    event: raw_events::PadEvent::Group {
                        group,
                        event: raw_events::PadGroupEvent::Strip {
                            strip: strip.id(),
                            event: crate::events::TouchStripEvent::Up,
                        },
                    },
                });
            }
            // ne
            _ => (),
        }
    }
}
