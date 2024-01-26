use wayland_backend::client::ObjectId;
use wayland_client::{
    protocol::{wl_registry, wl_seat},
    Connection, Dispatch, Proxy, QueueHandle,
};
use wayland_protocols::wp::tablet::zv2::client as wl_tablet;

use crate::{
    tablet::Tablet,
    tool::{AvailableAxes, Tool},
};
pub trait HasWlId {
    fn new_default(id: ObjectId) -> Self;
    fn id(&self) -> &ObjectId;
    /// Sent when constructors are done. Use this to
    /// make everything internally consistend.
    fn done(&mut self) {}
}
impl HasWlId for Tool {
    fn new_default(id: ObjectId) -> Self {
        Tool {
            obj_id: id,
            id: None,
            wacom_id: None,
            available_axes: AvailableAxes::empty(),
            tool_type: None,
            axis_info: Default::default(),
            distance_unit: crate::tool::DistanceUnit::Unitless,
        }
    }
    fn id(&self) -> &ObjectId {
        &self.obj_id
    }
}
impl HasWlId for Tablet {
    fn new_default(id: ObjectId) -> Self {
        Tablet {
            obj_id: id,
            name: String::new(),
        }
    }
    fn id(&self) -> &ObjectId {
        &self.obj_id
    }
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
pub(crate) struct WlTablets {
    pub(crate) collection: WlCollection<Tablet>,
}
#[derive(Default)]
pub(crate) struct WlTools {
    pub(crate) collection: WlCollection<Tool>,
}

#[derive(Default)]
pub struct TabletState {
    pub seat: Option<wl_seat::WlSeat>,
    pub manager: Option<wl_tablet::zwp_tablet_manager_v2::ZwpTabletManagerV2>,
    pub tablet_seat: Option<wl_tablet::zwp_tablet_seat_v2::ZwpTabletSeatV2>,
    pub(crate) tablets: WlTablets,
    pub(crate) tools: WlTools,
    // We use linear scans to store multiple tablets.
    // This simplifies the API at little cost, as we don't expect more than a handful at any given time.
    pub pending_events: Vec<()>,
}
impl TabletState {
    fn try_acquire_tablet_seat(&mut self, qh: &QueueHandle<Self>) {
        if self.tablet_seat.is_none() {
            if let Some((seat, tablet)) = self.seat.as_ref().zip(self.manager.as_ref()) {
                println!("Tablet server connected!");
                self.tablet_seat = Some(tablet.get_tablet_seat(seat, qh, ()));
            } else {
                println!("No seat to connect tablet server to.");
            }
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
        // When receiving events from the wl_registry, we are only interested in the
        // `global` event, which signals a new available global.
        // When receiving this event, we just print its characteristics in this example.
        println!("{event:?}");
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
            Event::Done => this.tablets.collection.done(&tablet.id()),
            Event::Id { .. } => (),
            Event::Name { name } => {
                this.tablets
                    .collection
                    .get_or_insert_ctor(tablet.id())
                    .unwrap()
                    .name = name;
            }
            Event::Path { .. } => (),
            Event::Removed => this.tablets.collection.destroy(&tablet.id()),
            // ne
            _ => (),
        }
    }
}
impl Dispatch<wl_tablet::zwp_tablet_pad_v2::ZwpTabletPadV2, ()> for TabletState {
    fn event(
        _: &mut Self,
        _pad: &wl_tablet::zwp_tablet_pad_v2::ZwpTabletPadV2,
        event: wl_tablet::zwp_tablet_pad_v2::Event,
        (): &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        // When receiving events from the wl_registry, we are only interested in the
        // `global` event, which signals a new available global.
        // When receiving this event, we just print its characteristics in this example.
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
impl Dispatch<wl_tablet::zwp_tablet_tool_v2::ZwpTabletToolV2, ()> for TabletState {
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
                let ctor = this.tools.collection.get_or_insert_ctor(tool.id()).unwrap();
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
                let ctor = this.tools.collection.get_or_insert_ctor(tool.id()).unwrap();
                ctor.wacom_id = Some(u64::from(hardware_id_hi) << 32 | u64::from(hardware_id_lo));
            }
            Event::HardwareSerial {
                hardware_serial_hi,
                hardware_serial_lo,
            } => {
                let ctor = this.tools.collection.get_or_insert_ctor(tool.id()).unwrap();
                ctor.id = Some(u64::from(hardware_serial_hi) << 32 | u64::from(hardware_serial_lo));
            }
            Event::Type {
                tool_type: wayland_client::WEnum::Value(tool_type),
            } => {
                use crate::tool::Type;
                use wl_tablet::zwp_tablet_tool_v2::Type as WlType;
                let ctor = this.tools.collection.get_or_insert_ctor(tool.id()).unwrap();
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
                this.tools.collection.done(&tool.id());
            }
            Event::Removed => this.tools.collection.destroy(&tool.id()),
            // ======== Interaction data =========
            // ne
            _ => (),
        }
    }
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
        // When receiving events from the wl_registry, we are only interested in the
        // `global` event, which signals a new available global.
        // When receiving this event, we just print its characteristics in this example.
        println!("pad group {event:?}");
    }
    wayland_client::event_created_child!(
        TabletState,
        wl_tablet::zwp_tablet_pad_group_v2::ZwpTabletPadGroupV2,
        [
            wl_tablet::zwp_tablet_pad_group_v2::EVT_RING_OPCODE => (wl_tablet::zwp_tablet_pad_ring_v2::ZwpTabletPadRingV2, ()),
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
        // When receiving events from the wl_registry, we are only interested in the
        // `global` event, which signals a new available global.
        // When receiving this event, we just print its characteristics in this example.
        println!("ring {event:?}");
    }
}
