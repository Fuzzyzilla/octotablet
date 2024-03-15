//! Implementation details for Wayland's `tablet_unstable_v2` protocol.
//!
//! Within this module, it is sound to assume `cfg(wl_tablet) == true`
//! (compiling for a wayland target + has deps, or is building docs).
use crate::{
    events::raw as raw_events,
    pad::{Group, Ring, Strip, TouchSource},
};
pub type ID = wayland_backend::client::ObjectId;
pub type ButtonID = u32;
use wayland_client::{
    protocol::{wl_registry, wl_seat},
    Connection, Dispatch, Proxy, QueueHandle,
};
use wayland_protocols::wp::tablet::zv2::client as wl_tablet;

use crate::{
    axis::Pose,
    events::FrameTimestamp,
    pad::Pad,
    tablet::{Tablet, UsbId},
    tool::Tool,
    util::NicheF32,
};

use super::InternalID;
pub struct Manager {
    _display: wayland_client::protocol::wl_display::WlDisplay,
    _conn: wayland_client::Connection,
    queue: wayland_client::EventQueue<TabletState>,
    _qh: wayland_client::QueueHandle<TabletState>,
    state: TabletState,
}

mod pad_impl;
mod summary;
mod tool_impl;

impl Manager {
    /// Creates a tablet manager with from the given pointer to `wl_display`.
    /// # Safety
    /// The given display pointer must be valid as long as the returned `Manager` is alive.
    pub(crate) unsafe fn build_wayland_display(
        _: crate::builder::Builder,
        wl_display: *mut (),
    ) -> Manager {
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
        self.state.raw_summary.make_concrete(self)
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
            name: None,
            hardware_id: None,
            wacom_id: None,
            tool_type: None,
            axes: crate::axis::FullInfo::default(),
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
            name: None,
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
    raw_summary: summary::Summary,
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
        // Remove last frame's events
        self.events.clear();
        // Exec all the defered destructors
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
        self.raw_summary.consume_oneshot();
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
                    button_pressure: NicheF32::NONE,
                    contact_size: None,
                };

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
                this.partial_tablets.get_or_insert_ctor(tablet.id()).name = Some(name);
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
