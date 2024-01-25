use raw_window_handle::HasDisplayHandle;
use wayland_client::{
    protocol::{wl_registry, wl_seat},
    Connection, Dispatch, Proxy, QueueHandle,
};
use wayland_protocols::wp::tablet::zv2::client as wl_tablet;
struct AppData {
    seat: Option<wl_seat::WlSeat>,
    tablet: Option<wl_tablet::zwp_tablet_manager_v2::ZwpTabletManagerV2>,
    tablet_seat: Option<wl_tablet::zwp_tablet_seat_v2::ZwpTabletSeatV2>,
}
impl Dispatch<wl_registry::WlRegistry, ()> for AppData {
    fn event(
        this: &mut Self,
        registry: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _: &(),
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
                    println!("Bound seat.");
                }
                "zwp_tablet_manager_v2" => {
                    this.tablet = Some(registry.bind(name, version, qh, ()));
                    println!("Bound tablet id {}", this.tablet.as_ref().unwrap().id());
                }
                _ => (),
            }
        }
    }
}
impl Dispatch<wl_seat::WlSeat, ()> for AppData {
    fn event(
        this: &mut Self,
        seat: &wl_seat::WlSeat,
        event: wl_seat::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wl_seat::Event::Capabilities {
            capabilities: wayland_client::WEnum::Value(capabilities),
        } = event
        {
            println!("Capabilities: {capabilities:?}");
        }

        // When receiving events from the wl_registry, we are only interested in the
        // `global` event, which signals a new available global.
        // When receiving this event, we just print its characteristics in this example.
    }
}
impl Dispatch<wl_tablet::zwp_tablet_manager_v2::ZwpTabletManagerV2, ()> for AppData {
    fn event(
        this: &mut Self,
        tablet: &wl_tablet::zwp_tablet_manager_v2::ZwpTabletManagerV2,
        event: wl_tablet::zwp_tablet_manager_v2::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        // NEVER ENTERS, DON'T PUT STUFF HERE
        unreachable!()
    }
}
impl Dispatch<wl_tablet::zwp_tablet_seat_v2::ZwpTabletSeatV2, ()> for AppData {
    fn event(
        _: &mut Self,
        seat: &wl_tablet::zwp_tablet_seat_v2::ZwpTabletSeatV2,
        event: wl_tablet::zwp_tablet_seat_v2::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        // When receiving events from the wl_registry, we are only interested in the
        // `global` event, which signals a new available global.
        // When receiving this event, we just print its characteristics in this example.
        println!("seat {event:?}");
    }
    wayland_client::event_created_child!(
        AppData,
        wl_tablet::zwp_tablet_seat_v2::ZwpTabletSeatV2,
        [
            wl_tablet::zwp_tablet_seat_v2::EVT_PAD_ADDED_OPCODE => (wl_tablet::zwp_tablet_pad_v2::ZwpTabletPadV2, ()),
            wl_tablet::zwp_tablet_seat_v2::EVT_TABLET_ADDED_OPCODE => (wl_tablet::zwp_tablet_v2::ZwpTabletV2, ()),
            wl_tablet::zwp_tablet_seat_v2::EVT_TOOL_ADDED_OPCODE => (wl_tablet::zwp_tablet_tool_v2::ZwpTabletToolV2, ()),
        ]
    );
}
impl Dispatch<wl_tablet::zwp_tablet_v2::ZwpTabletV2, ()> for AppData {
    fn event(
        _: &mut Self,
        seat: &wl_tablet::zwp_tablet_v2::ZwpTabletV2,
        event: wl_tablet::zwp_tablet_v2::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        // When receiving events from the wl_registry, we are only interested in the
        // `global` event, which signals a new available global.
        // When receiving this event, we just print its characteristics in this example.
        println!("tablet {event:?}");
    }
}
impl Dispatch<wl_tablet::zwp_tablet_pad_v2::ZwpTabletPadV2, ()> for AppData {
    fn event(
        _: &mut Self,
        seat: &wl_tablet::zwp_tablet_pad_v2::ZwpTabletPadV2,
        event: wl_tablet::zwp_tablet_pad_v2::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        // When receiving events from the wl_registry, we are only interested in the
        // `global` event, which signals a new available global.
        // When receiving this event, we just print its characteristics in this example.
        println!("pad {event:?}");
    }
    wayland_client::event_created_child!(
        AppData,
        wl_tablet::zwp_tablet_pad_v2::ZwpTabletPadV2,
        [
            wl_tablet::zwp_tablet_pad_v2::EVT_GROUP_OPCODE => (wl_tablet::zwp_tablet_pad_group_v2::ZwpTabletPadGroupV2, ()),
        ]
    );
}
impl Dispatch<wl_tablet::zwp_tablet_tool_v2::ZwpTabletToolV2, ()> for AppData {
    fn event(
        _: &mut Self,
        seat: &wl_tablet::zwp_tablet_tool_v2::ZwpTabletToolV2,
        event: wl_tablet::zwp_tablet_tool_v2::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        // When receiving events from the wl_registry, we are only interested in the
        // `global` event, which signals a new available global.
        // When receiving this event, we just print its characteristics in this example.
        println!("tool {event:?}");
    }
}
impl Dispatch<wl_tablet::zwp_tablet_pad_group_v2::ZwpTabletPadGroupV2, ()> for AppData {
    fn event(
        _: &mut Self,
        seat: &wl_tablet::zwp_tablet_pad_group_v2::ZwpTabletPadGroupV2,
        event: wl_tablet::zwp_tablet_pad_group_v2::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        // When receiving events from the wl_registry, we are only interested in the
        // `global` event, which signals a new available global.
        // When receiving this event, we just print its characteristics in this example.
        println!("pad group {event:?}");
    }
    wayland_client::event_created_child!(
        AppData,
        wl_tablet::zwp_tablet_pad_group_v2::ZwpTabletPadGroupV2,
        [
            wl_tablet::zwp_tablet_pad_group_v2::EVT_RING_OPCODE => (wl_tablet::zwp_tablet_pad_ring_v2::ZwpTabletPadRingV2, ()),
        ]
    );
}
impl Dispatch<wl_tablet::zwp_tablet_pad_ring_v2::ZwpTabletPadRingV2, ()> for AppData {
    fn event(
        _: &mut Self,
        seat: &wl_tablet::zwp_tablet_pad_ring_v2::ZwpTabletPadRingV2,
        event: wl_tablet::zwp_tablet_pad_ring_v2::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        // When receiving events from the wl_registry, we are only interested in the
        // `global` event, which signals a new available global.
        // When receiving this event, we just print its characteristics in this example.
        println!("ring {event:?}");
    }
}
fn create_tablet(d: &impl HasDisplayHandle, mut pump: impl FnMut() -> bool) -> Result<(), ()> {
    let d = d.display_handle().unwrap();
    let raw_window_handle::RawDisplayHandle::Wayland(d) = d.as_raw() else {
        return Err(());
    };

    // Safety - &impl Display borrowed for whole scope, so must survive that long.
    let backend = unsafe {
        wayland_backend::client::Backend::from_foreign_display(d.display.as_ptr() as *mut _)
    };
    let connection = wayland_client::Connection::from_backend(backend);
    let display = connection.display();
    let mut queue = connection.new_event_queue::<AppData>();
    let qh = queue.handle();

    let mut app = AppData {
        seat: None,
        tablet: None,
        tablet_seat: None,
    };
    let registry = display.get_registry(&qh, ());
    queue.roundtrip(&mut app).unwrap();

    while !pump() {
        queue.blocking_dispatch(&mut app).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(50));
        if app.tablet_seat.is_none() {
            if let Some((seat, tablet)) = app.seat.as_ref().zip(app.tablet.as_ref()) {
                println!("Tablet server connected!");
                app.tablet_seat = Some(tablet.get_tablet_seat(seat, &qh, ()));
            } else {
                println!("No seat to connect tablet server to.")
            }
        }
    }

    Ok(())
}

fn main() {
    use winit::platform::pump_events::EventLoopExtPumpEvents;
    let mut event_loop = winit::event_loop::EventLoopBuilder::<()>::default()
        .build()
        .unwrap();
    let window = Box::new(
        winit::window::WindowBuilder::default()
            .build(&event_loop)
            .unwrap(),
    );
    // Eeeeevil. But i don't wanna implement an RAII wrapper for wayland rn.
    // Will be cleaned up but OS at exit.... i think
    let window = Box::leak::<'static>(window);
    let softbuffer = softbuffer::Context::new(&*window).unwrap();
    let mut surface = softbuffer::Surface::new(&softbuffer, &*window).unwrap();

    let pump = || {
        event_loop.pump_events(Some(std::time::Duration::ZERO), |e, target| {
            use winit::event::*;
            match e {
                Event::WindowEvent {
                    event: WindowEvent::CloseRequested,
                    ..
                } => target.exit(),
                Event::WindowEvent {
                    event: WindowEvent::RedrawRequested,
                    ..
                } => {
                    let (width, height) = {
                        let size = window.inner_size();
                        (size.width, size.height)
                    };
                    surface
                        .resize(
                            std::num::NonZeroU32::new(width).unwrap(),
                            std::num::NonZeroU32::new(height).unwrap(),
                        )
                        .unwrap();
                    let mut buffer = surface.buffer_mut().unwrap();
                    buffer.fill(0);
                    buffer.present().unwrap();
                }
                Event::AboutToWait => {
                    window.request_redraw();
                }
                _ => (),
            }
        });

        event_loop.exiting()
    };

    create_tablet(window, pump).unwrap();

    event_loop.exit();
}
