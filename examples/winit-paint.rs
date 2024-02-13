//! Simple winit example, using tiny-skia for drawing.
//! Run under `--release` or it's horrifyingly slow!

use octotablet::builder::Builder;
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
    let mut builder = tiny_skia::PathBuilder::new();
    let softbuffer = softbuffer::Context::new(&window).unwrap();
    let mut surface = softbuffer::Surface::new(&softbuffer, &window).unwrap();

    // Which pen is currently `In`?
    let mut down_tool = None::<octotablet::tool::ID>;
    // Is the current tool an eraser?
    let mut is_eraser = false;
    // Current pressure. tiny_skia doesn't allow per-point size nor color, so we just choose per-frame.
    let mut current_pressure = 0.0;
    // If it was down, where was it at the end of the last frame?
    let mut previous_point = None::<[f32; 2]>;

    // Fetch the tablets, using our window's handle for access.
    // Since we `Arc'd` our window, we get the safety of `build_shared`. Where this is not possible,
    // `build_raw` is available as well!
    let mut manager = Builder::default().build_shared(window.clone()).unwrap();

    // Re-usable logic to draw and consume the path.
    let consume_path = |pixmap: &mut tiny_skia::Pixmap,
                        path: &mut tiny_skia::PathBuilder,
                        pressure: f32,
                        is_eraser: bool| {
        const BRUSH_SIZE: f32 = 40.0;
        // If empty or fails to build, skip.
        if path.is_empty() || pressure <= 0.0 {
            return;
        }
        let Some(built) = std::mem::take(path).finish() else {
            return;
        };

        // If the tool is known to be an eraser, erase!
        let mut color = if is_eraser {
            // mwuhahahaha, clear with black instead of actually reducing opacity.
            tiny_skia::Color::BLACK
        } else {
            tiny_skia::Color::WHITE
        };

        // Apply some arbitrary curve to make it nicer visually
        color.apply_opacity(pressure.powf(0.5));

        pixmap.stroke_path(
            &built,
            &tiny_skia::Paint {
                shader: tiny_skia::Shader::SolidColor(color),
                blend_mode: tiny_skia::BlendMode::SourceOver,
                anti_alias: false,
                force_hq_pipeline: false,
            },
            &tiny_skia::Stroke {
                width: pressure * BRUSH_SIZE,
                miter_limit: 4.0,
                line_cap: tiny_skia::LineCap::Round,
                line_join: tiny_skia::LineJoin::Round,
                dash: None,
            },
            tiny_skia::Transform::identity(),
            None,
        );

        // Pixbuf changed, ask winit logic to display it.
        window.request_redraw();
    };

    // Start pumping events...
    while !event_loop.exiting() {
        // Throttle the loop. Everything here will run *as fast as possible*
        // eating up a lot of CPU for no good purpose!
        // Fixme: it shouldn't do this by default. Something is wrong with my winit usage, i swear I've tried every
        // permutation of wait-times and control-flows and.... :(

        let wait_time = if previous_point.is_some() {
            // When drawing, poll often: This is not necessary to get the full quality out of `octotab` - quality is the same
            // regardless of polling rate when using the `event` api. However, this makes it feel smoother to draw.
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
        // We use the events API here as opposed to the summary API presented in the `eframe-viewer` example.
        // This is so that we can have the highest fidelity record of the digitizer movements, regardless of lag or framerate.
        // Events are reported upwards of 1000 per second, so much detail!
        let events = manager.pump().unwrap();
        for event in events {
            // We only care about tool events...
            if let octotablet::events::Event::Tool { tool, event } = event {
                // We're already listening on a different tool...
                if down_tool.as_ref().is_some_and(|t| t != &tool.id()) {
                    continue;
                }
                match event {
                    // Start listening! We don't start listening at "In", as we only care for motions that are pressed against the page.
                    octotablet::events::ToolEvent::Down => {
                        down_tool = Some(tool.id());
                        is_eraser = Some(octotablet::tool::Type::Eraser) == tool.tool_type;
                    }
                    octotablet::events::ToolEvent::Pose(pose) => {
                        // Ignore poses if the tool isn't down yet.
                        if down_tool.is_none() {
                            continue;
                        }
                        // strokes crossing many frames makes for more complex logic!
                        match (previous_point, builder.is_empty()) {
                            (None, _) => {
                                // Start of stroke, add move verb to position and don't line.
                                builder.move_to(pose.position[0], pose.position[1]);
                            }
                            (Some([prev_x, prev_y]), true) => {
                                // Stroke continued from last frame. Add a move verb to last point and then line
                                builder.move_to(prev_x, prev_y);
                                builder.line_to(pose.position[0], pose.position[1]);
                            }
                            (Some(_), false) => {
                                // Continuing from this frame. Just line.
                                builder.line_to(pose.position[0], pose.position[1]);
                            }
                        }
                        // Update last pos, in case this happens to be the last event.
                        previous_point = Some(pose.position);

                        // Choose a brush size from the pressure. Due to renderer limitations, this isn't
                        // part of the path and is coarsly chosen per-frame. FIXME! :D
                        current_pressure = pose.pressure.get().unwrap_or(1.0);
                    }
                    // Current interaction just stopped (hardware removed, out of proximity, or no longer pressed)
                    octotablet::events::ToolEvent::Removed
                    | octotablet::events::ToolEvent::Out
                    | octotablet::events::ToolEvent::Up => {
                        // Stop!
                        down_tool = None;
                        previous_point = None;
                        // Draw what we had in-progress..
                        consume_path(&mut pixmap, &mut builder, current_pressure, is_eraser);
                    }
                    // We don't care about the other tool events - buttons, frame times, ect.
                    _ => (),
                }
            }
        }
        // Draw what we had in-progress...
        consume_path(&mut pixmap, &mut builder, current_pressure, is_eraser);
    }
}
