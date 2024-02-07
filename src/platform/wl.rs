//! Implementation details for Wayland's `tablet_unstable_v2` protocol.
//!
//! Within this module, it is sound to assume `cfg(wl_tablet) == true`
//! (compiling for a wayland target + has deps, or is building docs).
use wayland_backend::client::ObjectId;
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
        &self.state.pads.finished
    }
    #[must_use]
    fn tools(&self) -> &[crate::tool::Tool] {
        &self.state.tools.finished
    }
    #[must_use]
    fn tablets(&self) -> &[crate::tablet::Tablet] {
        &self.state.tablets.finished
    }
    #[must_use]
    fn make_summary(&self) -> crate::events::summary::Summary {
        let try_summarize = || -> Option<crate::events::summary::Summary> {
            let sum = self.state.summary.clone()?;

            let tablet = self
                .tablets()
                .iter()
                .find(|tab| tab.obj_id.unwrap_wl() == &sum.tablet_id)?;
            let tool = self
                .tools()
                .iter()
                .find(|tab| tab.obj_id.unwrap_wl() == &sum.tool_id)?;
            Some(crate::events::summary::Summary {
                tool: crate::events::summary::ToolState::In(crate::events::summary::InState {
                    tablet,
                    tool,
                    pose: sum.pose,
                    down: sum.down,
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

pub trait HasWlId {
    fn new_default(id: ObjectId) -> Self;
    fn id(&self) -> &ObjectId;
    /// Sent when constructors are done. Use this to
    /// make everything internally consistent.
    fn done(&mut self) {}
}
impl HasWlId for Tool {
    fn new_default(id: ObjectId) -> Self {
        Tool {
            obj_id: InternalID::Wayland(id),
            id: None,
            wacom_id: None,
            available_axes: AvailableAxes::empty(),
            tool_type: None,
            // Unfortunately, Wayland doesn't enumerate axis precision info. :<
            axis_info: Default::default(),
            position_info: AxisInfo::default(),
            distance_unit: crate::tool::DistanceUnit::Unitless,
        }
    }
    fn id(&self) -> &ObjectId {
        self.obj_id.unwrap_wl()
    }
}
impl HasWlId for Tablet {
    fn new_default(id: ObjectId) -> Self {
        Tablet {
            obj_id: InternalID::Wayland(id),
            name: String::new(),
            usb_id: None,
        }
    }
    fn id(&self) -> &ObjectId {
        self.obj_id.unwrap_wl()
    }
}
impl HasWlId for Pad {
    fn new_default(id: ObjectId) -> Self {
        Pad {
            obj_id: InternalID::Wayland(id),
            // 0 is the default and what the protocol specifies should be used if
            // the constructor for this value is never sent.
            button_count: 0,
        }
    }
    fn id(&self) -> &ObjectId {
        self.obj_id.unwrap_wl()
    }
}

#[derive(Clone)]
pub struct RawSummary {
    pub tool_id: ObjectId,
    pub tablet_id: ObjectId,
    pub down: bool,
    pub pose: Pose,
    pub time: FrameTimestamp,
}

#[derive(thiserror::Error, Debug)]
enum WlConstructError {
    /// Reported when a wayland event describing construction parameters is recieved after
    /// the objects finalization. As far as I know, this indicates a server bug.
    #[error("an object with this id is already constructed")]
    AlreadyFinished,
}

/// Tracks objects which are created by a
/// data burst followed by "done".
pub(crate) struct WlCollection<T> {
    pub(crate) constructing: Vec<T>,
    pub(crate) finished: Vec<T>,
}
impl<T> Default for WlCollection<T> {
    fn default() -> Self {
        Self {
            constructing: vec![],
            finished: vec![],
        }
    }
}
impl<T: HasWlId> WlCollection<T> {
    fn get_or_insert_ctor(&mut self, id: ObjectId) -> Result<&mut T, WlConstructError> {
        if self.finished.iter().any(|obj| obj.id() == &id) {
            return Err(WlConstructError::AlreadyFinished);
        }
        let index = if let Some(found) = self.constructing.iter().position(|obj| obj.id() == &id) {
            found
        } else {
            self.constructing.push(T::new_default(id));
            self.constructing.len() - 1
        };

        Ok(&mut self.constructing[index])
    }
    fn done(&mut self, id: &ObjectId) {
        if let Some(finished_idx) = self.constructing.iter().position(|obj| obj.id() == id) {
            let mut finished_obj = self.constructing.remove(finished_idx);
            finished_obj.done();
            // Ensure no item of this id currently exists.
            self.finished.retain(|obj| obj.id() != id);
            self.finished.push(finished_obj);
        }
    }
    fn destroy(&mut self, id: &ObjectId) {
        self.constructing.retain(|obj| obj.id() != id);
        self.finished.retain(|obj| obj.id() != id);
    }
}

#[derive(Default)]
pub struct TabletState {
    pub seat: Option<wl_seat::WlSeat>,
    pub manager: Option<wl_tablet::zwp_tablet_manager_v2::ZwpTabletManagerV2>,
    pub tablet_seat: Option<wl_tablet::zwp_tablet_seat_v2::ZwpTabletSeatV2>,
    pub tablets: WlCollection<Tablet>,
    pub tools: WlCollection<Tool>,
    pub pads: WlCollection<Pad>,
    pub _groups: WlCollection<Pad>,
    pub summary: Option<RawSummary>,
    // We use linear scans to store multiple tablets.
    // This simplifies the API at little cost, as we don't expect more than a handful at any given time.
    pub pending_events: Vec<()>,
}
impl TabletState {
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
                    println!("Bound seat.");
                }
                "zwp_tablet_manager_v2" => {
                    this.manager = Some(registry.bind(name, version, qh, ()));
                    // Need a seat and a tablet manager to bind tablet seat.
                    this.try_acquire_tablet_seat(qh);
                    println!("Bound tablet id {}", this.manager.as_ref().unwrap().id());
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
            Event::Done => this.tablets.done(&tablet.id()),
            Event::Id { vid, pid } => {
                // Convert to u16s (have been crammed into u32s...) and set, if any.
                this.tablets.get_or_insert_ctor(tablet.id()).unwrap().usb_id = u16::try_from(vid)
                    .ok()
                    .zip(u16::try_from(pid).ok())
                    .map(|(vid, pid)| UsbId { vid, pid });
            }
            Event::Name { name } => {
                this.tablets.get_or_insert_ctor(tablet.id()).unwrap().name = name;
            }
            Event::Path { .. } => (),
            Event::Removed => this.tablets.destroy(&tablet.id()),
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
                let ctor = this.tools.get_or_insert_ctor(tool.id()).unwrap();
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
                let ctor = this.tools.get_or_insert_ctor(tool.id()).unwrap();
                ctor.wacom_id = Some(u64::from(hardware_id_hi) << 32 | u64::from(hardware_id_lo));
            }
            Event::HardwareSerial {
                hardware_serial_hi,
                hardware_serial_lo,
            } => {
                let ctor = this.tools.get_or_insert_ctor(tool.id()).unwrap();
                ctor.id = Some(u64::from(hardware_serial_hi) << 32 | u64::from(hardware_serial_lo));
            }
            Event::Type {
                tool_type: wayland_client::WEnum::Value(tool_type),
            } => {
                use crate::tool::Type;
                use wl_tablet::zwp_tablet_tool_v2::Type as WlType;
                let ctor = this.tools.get_or_insert_ctor(tool.id()).unwrap();
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
                this.tools.done(&tool.id());
            }
            Event::Removed => this.tools.destroy(&tool.id()),
            // ======== Interaction data =========
            Event::ProximityIn { tablet, .. } => {
                this.summary = Some(RawSummary {
                    tablet_id: tablet.id(),
                    tool_id: tool.id(),
                    down: false,
                    pose: Pose::default(),
                    time: FrameTimestamp::epoch(),
                });
            }
            Event::ProximityOut { .. } => {
                if this
                    .summary
                    .as_ref()
                    .is_some_and(|sum| sum.tool_id == tool.id())
                {
                    this.summary = None;
                }
            }
            Event::Down { .. } => {
                if let Some(summary) = &mut this.summary {
                    if summary.tool_id == tool.id() {
                        summary.down = true;
                    }
                }
            }
            Event::Up { .. } => {
                if let Some(summary) = &mut this.summary {
                    if summary.tool_id == tool.id() {
                        summary.down = false;
                    }
                }
            }
            Event::Motion { x, y } => {
                if let Some(summary) = &mut this.summary {
                    // shhh...
                    #[allow(clippy::cast_possible_truncation)]
                    if summary.tool_id == tool.id() {
                        summary.pose.position = [x as f32, y as f32];
                    }
                }
            }
            Event::Tilt { tilt_x, tilt_y } => {
                if let Some(summary) = &mut this.summary {
                    // shhh...
                    #[allow(clippy::cast_possible_truncation)]
                    if summary.tool_id == tool.id() {
                        summary.pose.tilt =
                            Some([(tilt_x as f32).to_radians(), (tilt_y as f32).to_radians()]);
                    }
                }
            }
            Event::Pressure { pressure } => {
                if let Some(summary) = &mut this.summary {
                    #[allow(clippy::cast_precision_loss)]
                    if summary.tool_id == tool.id() {
                        summary.pose.pressure =
                            NicheF32::new_some((pressure as f32) / 65535.0).unwrap();
                    }
                }
            }
            Event::Distance { distance } => {
                if let Some(summary) = &mut this.summary {
                    #[allow(clippy::cast_precision_loss)]
                    if summary.tool_id == tool.id() {
                        summary.pose.distance =
                            NicheF32::new_some((distance as f32) / 65535.0).unwrap();
                    }
                }
            }
            Event::Rotation { degrees } => {
                if let Some(summary) = &mut this.summary {
                    #[allow(clippy::cast_possible_truncation)]
                    if summary.tool_id == tool.id() {
                        summary.pose.roll =
                            NicheF32::new_some((degrees as f32).to_radians()).unwrap();
                    }
                }
            }
            Event::Slider { position } => {
                if let Some(summary) = &mut this.summary {
                    #[allow(clippy::cast_precision_loss)]
                    if summary.tool_id == tool.id() {
                        summary.pose.distance =
                            NicheF32::new_some((position as f32) / 65535.0).unwrap();
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
            Event::Group { .. } => (),
            Event::Path { .. } => (),
            Event::Buttons { buttons } => {
                let ctor = this.pads.get_or_insert_ctor(pad.id()).unwrap();
                ctor.button_count = buttons;
            }
            Event::Done => {
                this.pads.done(&pad.id());
            }
            Event::Removed => {
                this.pads.destroy(&pad.id());
            }
            // ======== Interaction data =========
            Event::Button { .. } => (),
            Event::Enter { .. } => (),
            Event::Leave { .. } => (),
            // ne
            _ => (),
        }
        println!("pad {event:?}");
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
        _: &mut Self,
        _group: &wl_tablet::zwp_tablet_pad_group_v2::ZwpTabletPadGroupV2,
        event: wl_tablet::zwp_tablet_pad_group_v2::Event,
        (): &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        println!("pad group {event:?}");
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
        _: &mut Self,
        _ring: &wl_tablet::zwp_tablet_pad_ring_v2::ZwpTabletPadRingV2,
        event: wl_tablet::zwp_tablet_pad_ring_v2::Event,
        (): &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        println!("ring {event:?}");
    }
}
impl Dispatch<wl_tablet::zwp_tablet_pad_strip_v2::ZwpTabletPadStripV2, ()> for TabletState {
    fn event(
        _: &mut Self,
        _ring: &wl_tablet::zwp_tablet_pad_strip_v2::ZwpTabletPadStripV2,
        event: wl_tablet::zwp_tablet_pad_strip_v2::Event,
        (): &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        println!("strip {event:?}");
    }
}
