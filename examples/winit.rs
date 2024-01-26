use raw_window_handle::HasRawDisplayHandle;

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
    let window = Box::leak::<'static>(window);

    // Some compositors require a window to query tablet info.
    // In order for us to see a window, we must present to it.
    let softbuffer = softbuffer::Context::new(&*window).unwrap();
    let mut surface = softbuffer::Surface::new(&softbuffer, &*window).unwrap();

    let mut manager = unsafe { wl_tablet::Manager::new_raw(window.raw_display_handle()) }.unwrap();
    while !event_loop.exiting() {
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
        manager.pump().unwrap();
    }
}
