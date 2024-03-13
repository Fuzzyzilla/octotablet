use eframe::{
    egui::{self, Frame, RichText},
    emath::Align2,
    epaint::{Color32, FontId, Shape, Stroke, Vec2},
    CreationContext,
};
use octotablet::{
    axis::AvailableAxes,
    builder::{BuildError, Builder},
    events::summary::{InState, Summary, ToolState},
    tablet::UsbId,
    Manager,
};

fn main() {
    let native_options = eframe::NativeOptions {
        persist_window: false,
        viewport: egui::ViewportBuilder::default().with_inner_size(Vec2 { x: 800.0, y: 500.0 }),
        // Im stupid and don't want to figure out how to make the
        // colors dynamic, they only work on Dark lol
        default_theme: eframe::Theme::Dark,
        ..Default::default()
    };
    // Startup!
    eframe::run_native(
        "example-tablet-viewer",
        native_options,
        Box::new(|context| Box::new(Viewer::new(context))),
    )
    .unwrap();
}

/// Main app, displaying info and a [test area](ShowPen).
struct Viewer {
    manager: Result<Manager, BuildError>,
    // Keep track of when the last frame was, so that we can show speed of events
    last_frame_time: std::time::Instant,
    // Allow code to set a duration to poll events more rapidly.
    // Egui does not automatically update when the tablet does!
    poll_until: std::time::Instant,
    // Show a toggle for whether to show the event queue, off by default.
    show_events: bool,
    // The event queue in question:
    events_stream: std::collections::VecDeque<(String, Color32)>,
    // a number that the user can scroll with rings or sliders.
    pad_scroll: f32,
}
impl Viewer {
    fn new(context: &CreationContext<'_>) -> Self {
        let now = std::time::Instant::now();
        // Context gives us access to the handle, connect to the tablet server:
        Self {
            // Safety: Destroyed in `on_exit`, before we lose the display.
            manager: unsafe { Builder::new().build_raw(context) },
            last_frame_time: now,
            poll_until: now,
            show_events: false,
            events_stream: std::collections::VecDeque::with_capacity(128),
            pad_scroll: 0.0,
        }
    }
}
impl eframe::App for Viewer {
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        // Drop the tablet, since our connection to the server is soon over.
        // Replace with dummy err.
        self.manager = Err(BuildError::Unsupported);
    }
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Access tablets, or show a message and bail if failed.
        let manager = match &mut self.manager {
            Ok(t) => t,
            Err(e) => {
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.label(
                        RichText::new(format!("Failed to acquire connection: {e}"))
                            .monospace()
                            .heading(),
                    )
                });
                return;
            }
        };
        let events = manager.pump().unwrap();
        let summary = events.summarize();
        // If at least one event...
        let has_events = events.into_iter().next().is_some();
        // Poll more often for a while!
        let poll = if has_events {
            // Set up to poll for an additional 500ms afterwards, to catch future events.
            self.poll_until = std::time::Instant::now() + std::time::Duration::from_millis(500);
            true
        } else {
            // No new events. Still poll if we're in the time frame set before:
            self.poll_until > std::time::Instant::now()
        };

        // If an interaction is ongoing, request redraws often.
        if poll {
            // Arbitrary fast poll time, without being so fast as to gobble up the CPU.
            ctx.request_repaint_after(std::time::Duration::from_secs_f32(1.0 / 60.0));
        } else {
            // poll for new events slower. (Egui will not necessarily notice the tablet input and so won't repaint on its own!)
            ctx.request_repaint_after(std::time::Duration::from_millis(250));
        }

        // Format events.
        let mut event_count = 0;
        for event in events {
            event_count += 1;
            if self.events_stream.len() == self.events_stream.capacity() {
                // remove top
                let _ = self.events_stream.pop_front();
            }
            // add pretty-print at bottom!
            self.events_stream.push_back(pretty_print_event(event));
        }
        // Show an area to print events, if requested.
        egui::SidePanel::right("events").show_animated(ctx, self.show_events, |ui| {
            // Show speed:
            let freq = event_count as f32 / self.last_frame_time.elapsed().as_secs_f32();
            ui.label(
                RichText::new(format!("{freq:.01} events/sec",))
                    .heading()
                    .weak(),
            );
            // Show events:
            egui::ScrollArea::new([false, true])
                .stick_to_bottom(true)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    egui::Grid::new("events-grid")
                        .num_columns(1)
                        .striped(true)
                        .show(ui, |ui| {
                            for (message, color) in &self.events_stream {
                                ui.label(RichText::new(message).monospace().color(*color));
                                ui.end_row();
                            }
                        });
                });
        });

        // Show an area to test various axes
        egui::TopBottomPanel::bottom("viewer")
            .exact_height(ctx.available_rect().height() / 2.0)
            .frame(Frame::canvas(&ctx.style()))
            .show(ctx, |ui| {
                ui.add(ShowPen {
                    summary,
                    pad_scroll: &mut self.pad_scroll,
                })
            });

        // Show an info panel listing connected devices and their capabilities
        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical()
                .auto_shrink(false)
                .show(ui, |ui| {
                    // Heading
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Octotablet viewer ~ Connected! 🐙").heading());
                        ui.toggle_value(&mut self.show_events, "Show Events stream...")
                    });
                    ui.separator();

                    ui.label("Tablets");
                    for (idx, tablet) in manager.tablets().iter().enumerate() {
                        egui::CollapsingHeader::new(
                            tablet.name.as_deref().unwrap_or("Unknown Tablet"),
                        )
                        .id_source((&tablet.name, idx))
                        .show(ui, |ui| {
                            // Pretty-print the USBID
                            ui.label(RichText::new(format_id(tablet.usb_id)).monospace())
                        });
                    }
                    if manager.tablets().is_empty() {
                        ui.label(RichText::new("No tablets...").weak());
                    }

                    ui.separator();

                    ui.label("Pads");
                    for (idx, pad) in manager.pads().iter().enumerate() {
                        ui.collapsing(idx.to_string(), |ui| {
                            ui.label(format!("Total Buttons: {}", pad.total_buttons));
                            for (idx, group) in pad.groups.iter().enumerate() {
                                egui::CollapsingHeader::new(format!("Group {idx}"))
                                    .default_open(true)
                                    .show(ui, |ui| {
                                        ui.label(format!(
                                            "Mode count: {:?}",
                                            group.mode_count.map(std::num::NonZeroU32::get)
                                        ));
                                        ui.label(format!(
                                            "Associated button indices: {:?}",
                                            &group.buttons
                                        ));
                                        // Show rings
                                        egui::CollapsingHeader::new(format!(
                                            "Rings ({})",
                                            group.rings.len()
                                        ))
                                        .default_open(true)
                                        .enabled(!group.rings.is_empty())
                                        .show(ui, |ui| {
                                            for ring in &group.rings {
                                                ui.label(format!("{ring:#?}"));
                                            }
                                        });
                                        // Show strips
                                        egui::CollapsingHeader::new(format!(
                                            "Strips ({})",
                                            group.strips.len()
                                        ))
                                        .default_open(true)
                                        .enabled(!group.strips.is_empty())
                                        .show(ui, |ui| {
                                            for strip in &group.strips {
                                                ui.label(format!("{strip:#?}"));
                                            }
                                        });
                                    });
                            }
                        });
                    }
                    if manager.pads().is_empty() {
                        ui.label(RichText::new("No pads...").weak());
                    }

                    ui.separator();

                    ui.label("Tools");
                    for (idx, tool) in manager.tools().iter().enumerate() {
                        let type_name = tool
                            .tool_type
                            .as_ref()
                            .map_or("Unknown tool", |ty| ty.as_ref());
                        let name = if let Some(id) = tool.hardware_id {
                            format!(
                                "{type_name:<7} ({:08X?} - Hardware ID: {id:08X})",
                                tool.id()
                            )
                        } else {
                            format!("{type_name:<7} ({:08X?} - Hardware ID Unknown", tool.id())
                        };
                        egui::CollapsingHeader::new(RichText::new(name).monospace())
                            .id_source((tool.hardware_id, tool.wacom_id, idx))
                            .show(ui, |ui| {
                                ui.label(format!("Wacom ID: {:08X?}", tool.wacom_id,));
                                ui.label(format!(" ✅ Position: {:?}", tool.axes.position));
                                for axis in
                                    <octotablet::axis::Axis as strum::IntoEnumIterator>::iter()
                                {
                                    // Todo: list units lol

                                    if let Ok((limits, granularity)) =
                                        tool.axes.limits(axis).and_then(|limits| {
                                            Ok((limits, tool.axes.granularity(axis)?))
                                        })
                                    {
                                        ui.label(format!(
                                            " ✅ {}: {}, {}",
                                            axis.as_ref(),
                                            match limits {
                                                Some(limits) => format!("{limits:?}"),
                                                None => "Unknown range".to_owned(),
                                            },
                                            match granularity {
                                                Some(granularity) => format!("{granularity:?}"),
                                                None => "Unknown granularity".to_owned(),
                                            },
                                        ));
                                    } else {
                                        ui.label(
                                            RichText::new(format!(
                                                " 🗙 {}: Unsupported",
                                                axis.as_ref()
                                            ))
                                            .weak()
                                            .italics(),
                                        );
                                    }
                                }
                            });
                    }
                    if manager.tools().is_empty() {
                        ui.label(RichText::new("No tools...").weak());
                    }

                    ui.separator();
                });
        });

        // Keep track of time
        self.last_frame_time = std::time::Instant::now();
    }
}

/// Uses a USB ID database to fetch info strings:
fn format_id(id: Option<UsbId>) -> String {
    use usb_ids::FromId;
    if let Some(id @ UsbId { vid, pid }) = id {
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
fn pretty_print_touch_event(
    name: String,
    event: octotablet::events::TouchStripEvent,
    fmt_pose: impl FnOnce(f32) -> String,
) -> (String, Color32) {
    match event {
        octotablet::events::TouchStripEvent::Pose(pose) => {
            (format!("{name} Pose {}", fmt_pose(pose)), Color32::WHITE)
        }
        octotablet::events::TouchStripEvent::Frame(time) => {
            if let Some(time) = time {
                (format!("{name} Frame {time:?}"), Color32::GRAY)
            } else {
                (format!("{name} Frame"), Color32::GRAY)
            }
        }
        octotablet::events::TouchStripEvent::Source(src) => {
            (format!("{name} Interacted by {src:?}"), Color32::GRAY)
        }
        octotablet::events::TouchStripEvent::Up => (format!("{name} Up"), Color32::LIGHT_BLUE),
    }
}

// Print out event in a nicer way than `Debug`, with some color too!
fn pretty_print_event(event: octotablet::events::Event) -> (String, Color32) {
    match event {
        octotablet::events::Event::Tablet { .. } => {
            // default format
            (format!("{event:#?}"), Color32::WHITE)
        }
        octotablet::events::Event::Pad { pad, event } => {
            let id = pad.id();
            match event {
                octotablet::events::PadEvent::Added => {
                    (format!("Added pad {id:08X?}"), Color32::GREEN)
                }
                octotablet::events::PadEvent::Removed => {
                    (format!("Removed pad {id:08X?}"), Color32::RED)
                }
                octotablet::events::PadEvent::Enter { tablet } => {
                    let tab_id = tablet.id();
                    (
                        format!("Pad {id:08X?} connected to {tab_id:08X?}"),
                        Color32::YELLOW,
                    )
                }
                octotablet::events::PadEvent::Exit => (
                    format!("Pad {id:08X?} disconnected from tablet"),
                    Color32::BROWN,
                ),
                octotablet::events::PadEvent::Button {
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
                            "Pad {id:08X?} Button {button_idx} {} (Owned by group {group_idx:?})",
                            if pressed { "Pressed" } else { "Released" }
                        ),
                        if pressed {
                            Color32::DARK_GREEN
                        } else {
                            Color32::DARK_RED
                        },
                    )
                }
                octotablet::events::PadEvent::Group { group, event } => {
                    let group = group.id();
                    match event {
                        octotablet::events::PadGroupEvent::Mode(m) => (
                            format!("Group {group:08X?} switched to mode {m}"),
                            Color32::LIGHT_BLUE,
                        ),
                        octotablet::events::PadGroupEvent::Ring { ring, event } => {
                            let ring = ring.id();
                            pretty_print_touch_event(format!("Ring {ring:08X?}"), event, |pose| {
                                format!("{:.01}deg", pose.to_degrees())
                            })
                        }
                        octotablet::events::PadGroupEvent::Strip { strip, event } => {
                            let strip = strip.id();
                            pretty_print_touch_event(format!("Strip {strip:08X?}"), event, |pose| {
                                format!("{:.01}%", pose * 100.0)
                            })
                        }
                    }
                }
            }
        }
        octotablet::events::Event::Tool { tool, event } => {
            let id = tool.id();
            match event {
                octotablet::events::ToolEvent::Added => {
                    (format!("Added tool {id:08X?}"), Color32::GREEN)
                }
                octotablet::events::ToolEvent::Removed => {
                    (format!("Removed tool {id:08X?}"), Color32::RED)
                }
                octotablet::events::ToolEvent::In { tablet } => {
                    let tab_id = tablet.id();
                    (
                        format!("Tool {id:08X?} in\n  - over {tab_id:08X?}"),
                        Color32::YELLOW,
                    )
                }
                octotablet::events::ToolEvent::Out => {
                    (format!("Tool {id:08X?} out"), Color32::BROWN)
                }
                octotablet::events::ToolEvent::Down => {
                    (format!("Tool {id:08X?} down"), Color32::LIGHT_BLUE)
                }
                octotablet::events::ToolEvent::Up => {
                    (format!("Tool {id:08X?} up"), Color32::LIGHT_BLUE)
                }
                octotablet::events::ToolEvent::Pose(pose) => {
                    (format!("Tool {id:08X?} {pose:#?}"), Color32::WHITE)
                }
                octotablet::events::ToolEvent::Button { button_id, pressed } => (
                    format!(
                        "Tool {id:08X?} Button {button_id} {}",
                        if pressed { "Pressed" } else { "Released" }
                    ),
                    if pressed {
                        Color32::DARK_GREEN
                    } else {
                        Color32::DARK_RED
                    },
                ),
                octotablet::events::ToolEvent::Frame(time) => {
                    if let Some(time) = time {
                        (format!("Frame {time:?}"), Color32::GRAY)
                    } else {
                        ("Frame".into(), Color32::GRAY)
                    }
                }
            }
        }
    }
}

/// Pen test area, showing off collected event data visually.
struct ShowPen<'a> {
    summary: Summary<'a>,
    pad_scroll: &'a mut f32,
}
impl egui::Widget for ShowPen<'_> {
    fn ui(self, ui: &mut egui::Ui) -> egui::Response {
        let Self {
            summary,
            pad_scroll,
        } = self;
        let (resp, painter) = ui.allocate_painter(
            ui.available_size(),
            egui::Sense {
                click: false,
                drag: false,
                focusable: false,
            },
        );

        // Just play with the axes a bit to make something look nice :3
        // All of the math fiddling and constants are just for aesthetics, little of it means anything lol
        // Visualize as much as we can so we can tell at a glance everything is working.
        if let ToolState::In(state) = summary.tool {
            // ======= Interation text ======
            {
                // Show the name
                let mut cursor = resp.rect.left_top();
                let pen_name = match (state.tool.hardware_id, state.tool.tool_type) {
                    (None, None) => "Unknown Tool".to_owned(),
                    (Some(id), None) => format!("{:08X}", id),
                    (None, Some(ty)) => ty.as_ref().to_string(),
                    (Some(id), Some(ty)) => format!("{} {:08X}", ty.as_ref(), id),
                };
                let rect = painter.text(
                    cursor,
                    Align2::LEFT_TOP,
                    format!(
                        "{pen_name} on {}",
                        state.tablet.name.as_deref().unwrap_or("Unknown Tablet")
                    ),
                    FontId::default(),
                    Color32::WHITE,
                );
                cursor.y += rect.height();
                // I can't visualize tools I can't test :V
                // Let the user know if there's more available that are invisible.
                let available_axes = state.tool.axes.available();
                let visualized_axes =
                    AvailableAxes::PRESSURE | AvailableAxes::TILT | AvailableAxes::DISTANCE;
                let seen_axes = available_axes.intersection(visualized_axes);
                let not_seen_axes = available_axes.difference(visualized_axes);
                let axes = if !not_seen_axes.is_empty() {
                    // Pen supports axes not visualized.
                    format!(
                        "Showing axes: {seen_axes:?}. No visualizers for {not_seen_axes:?}, Fixme!"
                    )
                } else {
                    // We show all axes! yay!
                    format!("Showing all axes: {seen_axes:?}")
                };
                // Show the capabilities
                let rect = painter.text(
                    cursor,
                    Align2::LEFT_TOP,
                    axes,
                    FontId::default(),
                    Color32::WHITE,
                );
                cursor.y += rect.height();
                // Show pressed buttons
                let buttons = if !state.pressed_buttons.is_empty() {
                    format!("Pressing tool buttons {:04X?}", state.pressed_buttons)
                } else {
                    "No pressed tool buttons".to_owned()
                };
                painter.text(
                    cursor,
                    Align2::LEFT_TOP,
                    buttons,
                    FontId::default(),
                    Color32::WHITE,
                );
            }
            // ======= Visualize Pose ======
            ui.ctx().set_cursor_icon(egui::CursorIcon::None);
            // Fade out with distance.
            let opacity = state
                .pose
                .distance
                .get()
                .map(|v| 1.0 - v.powf(1.5))
                .unwrap_or(1.0);

            // Show an arrow in the direction of tilt.
            let tip_pos = egui::Pos2 {
                x: state.pose.position[0],
                y: state.pose.position[1],
            };
            if let Some([tiltx, tilty]) = state.pose.tilt {
                // These ops actually DO have mathematical meaning unlike the other
                // mad aesthetical tinkering. These tilts describe a 3D vector from tip to back of pen,
                // project this vector down onto the page for visualization.
                let mut tilt_vec = egui::Vec2 {
                    x: tiltx.sin(),
                    y: tilty.sin(),
                };
                // Hardware quirk - some devices report angles which don't make any physical or mathematical
                // sense. (trigonometrically, this length should always be <= 1. This is not the case in practice lol)
                if tilt_vec.length_sq() > 1.0 {
                    tilt_vec = tilt_vec.normalized();
                }
                painter.arrow(
                    tip_pos,
                    400.0 * tilt_vec,
                    Stroke {
                        color: Color32::WHITE
                            .gamma_multiply(tilt_vec.length())
                            .gamma_multiply(opacity),
                        width: tilt_vec.length() * 5.0,
                    },
                )
            }
            // Draw a shape to visualize pressure and distance and such
            painter.add(make_pen_shape(&state, opacity));
        } else {
            painter.text(
                resp.rect.center(),
                Align2::CENTER_CENTER,
                "Bring tool near...",
                FontId::new(30.0, Default::default()),
                Color32::WHITE,
            );
            ui.ctx().set_cursor_icon(egui::CursorIcon::Default);
        }
        // Pad stuffs!
        // ============ Show a scroll wheel based on rings and strips, with absolute touch positions ==============
        {
            const SCROLL_CIRCLE_SIZE: f32 = 50.0;
            const ABSOLUTE_CIRCLE_SIZE: f32 = 10.0;

            let mut interacted = false;
            let mut absolute_ring = None;
            let mut absolute_strip = None;
            // For every slider (ring or strip), update the delta.
            for pad in &summary.pads {
                for group in &pad.groups {
                    for ring in &group.rings {
                        if let Some(delta) = ring.delta_radians {
                            *pad_scroll += delta;
                            interacted = true;
                        }
                        // Pressed and absolute angle known
                        if ring.touched_by.is_some() && ring.angle.is_some() {
                            absolute_ring = Some(ring.angle.unwrap());
                        }
                    }
                    for strip in &group.strips {
                        if let Some(delta) = strip.delta {
                            *pad_scroll += delta;
                            interacted = true;
                        }
                        // Pressed and absolute position known
                        if strip.touched_by.is_some() && strip.position.is_some() {
                            absolute_strip = Some(strip.position.unwrap());
                        }
                    }
                }
            }

            let interacted =
                ui.ctx()
                    .animate_bool_with_time(egui::Id::new("scroll-opacity"), interacted, 2.0);

            let color = Color32::WHITE.gamma_multiply(interacted * 0.75 + 0.25);
            let stroke = Stroke::new(interacted * 3.0 + 2.0, color);

            // 0 is north, radians CW
            // Convert to 0 is East, radians CW.
            let angled =
                Vec2::angled(-std::f32::consts::FRAC_PI_2 + *pad_scroll) * SCROLL_CIRCLE_SIZE;
            // bottom corner with margin.
            let center = resp.rect.expand(-SCROLL_CIRCLE_SIZE * 1.2).left_bottom();
            // Base circle shape
            painter.circle_stroke(center, SCROLL_CIRCLE_SIZE, stroke);
            // Line showing current scroll position
            painter.line_segment([center + angled, center + angled * 0.5], stroke);
            // Circle showing absolute touch position
            if let Some(absolute_ring) = absolute_ring {
                let angled = Vec2::angled(-std::f32::consts::FRAC_PI_2 + absolute_ring)
                    * (SCROLL_CIRCLE_SIZE - ABSOLUTE_CIRCLE_SIZE);
                painter.circle_filled(center + angled * 0.9, ABSOLUTE_CIRCLE_SIZE, color);
            }
            // Vertical slider showing absolute strip position. (we don't know if this is a horizontal or
            // vertical strip, just gotta make this assumption)
            if let Some(absolute_strip) = absolute_strip {
                // Start to the top right of the scroll circle
                let start_pos = center
                    + Vec2::new(
                        SCROLL_CIRCLE_SIZE * 1.1,
                        -SCROLL_CIRCLE_SIZE + ABSOLUTE_CIRCLE_SIZE,
                    );
                // Move down two units over a swipe.
                let length_vec =
                    Vec2::new(0.0, SCROLL_CIRCLE_SIZE * 2.0 - ABSOLUTE_CIRCLE_SIZE * 2.0);

                painter.line_segment([start_pos, start_pos + length_vec], stroke);
                painter.circle_filled(
                    start_pos + absolute_strip * length_vec,
                    ABSOLUTE_CIRCLE_SIZE,
                    color,
                );
            }
        }
        // ================ Show buttons ===================
        {
            const BUTTON_CIRCLE_SIZE: f32 = 15.0;
            // Each pad with buttons gets it's own column in a square grid
            for (x, buttons) in summary
                .pads
                .iter()
                .filter_map(|pad| (!pad.buttons.is_empty()).then_some(&pad.buttons))
                .enumerate()
            {
                // Each button goes down in the column
                for (y, button) in buttons.iter().enumerate() {
                    let center = resp.rect.right_top()
                        + Vec2::new(-(x as f32 + 0.5), y as f32 + 0.5) * BUTTON_CIRCLE_SIZE * 2.5;
                    painter.add(if button.currently_pressed {
                        Shape::circle_filled(center, BUTTON_CIRCLE_SIZE, Color32::WHITE)
                    } else {
                        Shape::circle_stroke(
                            center,
                            BUTTON_CIRCLE_SIZE,
                            Stroke::new(2.0, Color32::from_white_alpha(128)),
                        )
                    });
                }
            }
        }
        resp
    }
}

/// Draw a circle around the pen visualizing distance, down, and pressure attributes.
fn make_pen_shape(state: &InState, opacity: f32) -> Shape {
    // White filled if pressed, red outline if hovered.
    let (fill, stroke) = if state.down {
        (
            Color32::WHITE.gamma_multiply(opacity),
            Stroke {
                width: 0.0,
                color: Color32::TRANSPARENT,
            },
        )
    } else {
        (
            Color32::TRANSPARENT,
            Stroke {
                width: 2.0,
                color: Color32::LIGHT_RED.gamma_multiply(opacity),
            },
        )
    };
    let radius = 75.0
        * if state.down {
            // Touching, show a dot sized by pressure.
            state.pose.pressure.get().unwrap_or(0.5)
        } else {
            // If not touching, show a large dot on hover
            // that closes in as the pen gets closer
            state
                .pose
                .distance
                .get()
                .map(|v| v.powf(1.5) * 0.5)
                .unwrap_or(0.5)
        };

    let tip_pos = egui::Pos2 {
        x: state.pose.position[0],
        y: state.pose.position[1],
    };

    Shape::Circle(eframe::epaint::CircleShape {
        center: tip_pos,
        radius,
        fill,
        stroke,
    })
}
