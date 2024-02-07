//! Simple winit example, using tiny-skia for drawing.
//! Run under `--release` or it's horrifyingly slow!

use octotablet::{builder::Builder, events::summary::ToolState, tool::Type};
use winit::dpi::PhysicalSize;

use winit::platform::pump_events::EventLoopExtPumpEvents;

fn main() {
    let mut event_loop = winit::event_loop::EventLoopBuilder::<()>::default()
        .build()
        .unwrap();
    let window = std::sync::Arc::new(
        winit::window::WindowBuilder::default()
            .with_inner_size(PhysicalSize::new(512u32, 512u32))
            .build(&event_loop)
            .unwrap(),
    );

    // To allow us to draw on the screen without pulling in a whole GPU package,
    // we use `softbuffer` for presentation and `tiny-skia` for drawing
    let mut pixmap = tiny_skia::Pixmap::new(512, 512).unwrap();
    let softbuffer = softbuffer::Context::new(&window).unwrap();
    let mut surface = softbuffer::Surface::new(&softbuffer, &window).unwrap();

    let mut previous_point = None::<[f32; 2]>;

    // Fetch the tablets, using our window's handle for access.
    // Since we `Arc'd` our window, we get the safety of `build_shared`. Where this is not possible,
    // `build_raw` is available as well!
    let mut manager = Builder::default().build_shared(window.clone()).unwrap();

    while !event_loop.exiting() {
        // Throttle the loop. Everything here will run *as fast as possible*
        // eating up a lot of CPU for no good purpose!
        // Fixme: it shouldn't do this by default. Something is wrong with my winit usage, i swear I've tried every
        // permutation of wait-times and control-flows and.... :(
        let wait_time = if previous_point.is_some() {
            // When drawing, poll often:
            std::time::Duration::from_millis(10)
        } else {
            // When not, poll less often. Can't be too long or `winit` gets unhappy!
            std::time::Duration::from_millis(50)
        };
        std::thread::sleep(wait_time);

        // Let winit manage its messages....
        event_loop.pump_events(Some(std::time::Duration::ZERO), |e, target| {
            use winit::event::*;
            // Use poll, since wait times are set *outside* the loop.
            // Wait stalls the thread, and ignores the `wait_time` parameter
            // for some reason!
            target.set_control_flow(winit::event_loop::ControlFlow::Poll);

            if let Event::WindowEvent { event, .. } = e {
                match event {
                    WindowEvent::CloseRequested => target.exit(),
                    WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                        if scale_factor != 1.0 {
                            // Nothing to test this on, so it's hard to write the transform math... Fixme!
                            unimplemented!("I don't know what math to put here :<")
                        }
                    }
                    WindowEvent::Resized(size) => {
                        // Make a new map
                        let mut new_map = tiny_skia::Pixmap::new(size.width, size.height).unwrap();
                        // copy the old onto it.
                        new_map.draw_pixmap(
                            0,
                            0,
                            pixmap.as_ref(),
                            &tiny_skia::PixmapPaint {
                                opacity: 1.0,
                                blend_mode: tiny_skia::BlendMode::Source,
                                quality: tiny_skia::FilterQuality::Nearest,
                            },
                            tiny_skia::Transform::identity(),
                            None,
                        );
                        // Replace map, inform the surface of the change, and redraw.
                        pixmap = new_map;
                        surface
                            .resize(
                                size.width.try_into().unwrap(),
                                size.height.try_into().unwrap(),
                            )
                            .unwrap();
                        window.request_redraw();
                    }
                    WindowEvent::RedrawRequested => {
                        // Copy skia bitmap into the framebufer and present.
                        let mut buffer = surface.buffer_mut().unwrap();
                        buffer
                            .iter_mut()
                            .zip(pixmap.pixels().iter())
                            .for_each(|(into, from)| {
                                // This is a premul color. Treat it as composited atop black,
                                // which has the nice side effect of requiring literally no work
                                // other than setting alpha to 1 :D
                                let r = from.red();
                                let g = from.green();
                                let b = from.blue();
                                // softbuffer requires `0000'0000'rrrr'rrrr'gggg'gggg'bbb'bbbb` format
                                *into = (u32::from(r) << 16) | (u32::from(g) << 8) | (u32::from(b));
                            });
                        buffer.present().unwrap();
                    }
                    _ => (),
                }
            }
        });

        // Accept all new messages from the stylus server.
        let sum = manager.pump().unwrap().summarize();
        // Todo: make this use events api instead of summary api, as is more correct for drawing app usecases.
        // If the pen is down, draw some pretty pictures! We do this by saving the last
        // known point, and drawing a line between that point and the current point.
        if let ToolState::In(state) = sum.tool {
            const BRUSH_SIZE: f32 = 20.0;
            // Ignore when not pressed.
            if !state.down {
                previous_point = None;
                continue;
            }
            // Get the las known point, or set it and bail for next loop around if not available.
            let Some([px, py]) = previous_point else {
                previous_point = Some(state.pose.position);
                continue;
            };

            // If the tool is known to be an eraser, erase!
            let mut color = if let Some(Type::Eraser) = state.tool.tool_type {
                // mwuhahahaha, clear with black instead of actually reducing opacity.
                tiny_skia::Color::BLACK
            } else {
                tiny_skia::Color::WHITE
            };

            // Opacity and size go up with force!
            let power = state.pose.pressure.get().unwrap_or(1.0);
            let opacity = power.clamp(0.0, 1.0);
            let size = power * BRUSH_SIZE;
            color.apply_opacity(opacity);

            // Line from last point to this point
            let [x, y] = state.pose.position;
            let mut path = tiny_skia::PathBuilder::with_capacity(2, 2);
            path.move_to(px, py);
            path.line_to(x, y);

            pixmap.stroke_path(
                &path.finish().unwrap(),
                &tiny_skia::Paint {
                    shader: tiny_skia::Shader::SolidColor(color),
                    // We fake opacity by using `Source` here, it essentially treats our transparent white as shades of grey.
                    // Real opacity doesn't look good because previous line stacks atop new ones at the ends...
                    blend_mode: tiny_skia::BlendMode::Source,
                    anti_alias: false,
                    force_hq_pipeline: false,
                },
                &tiny_skia::Stroke {
                    width: size,
                    miter_limit: 4.0,
                    line_cap: tiny_skia::LineCap::Round,
                    line_join: tiny_skia::LineJoin::Round,
                    dash: None,
                },
                tiny_skia::Transform::identity(),
                None,
            );

            // Update last point, refresh screen:
            previous_point = Some(state.pose.position);
            window.request_redraw();
        } else {
            // Not pressed, clear data.
            previous_point = None;
        }
    }
}
