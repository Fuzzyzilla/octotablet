use eframe::{
    egui::{self, Frame, RichText},
    emath::Align2,
    epaint::{Color32, FontId, Shape, Stroke, Vec2},
    CreationContext,
};
use raw_window_handle::HasDisplayHandle;
use wl_tablet::{
    events::summary::{InState, Summary},
    tablet::UsbId,
    tool::{AvailableAxes, Axis},
};

fn main() {
    let native_options = eframe::NativeOptions {
        persist_window: false,
        viewport: egui::ViewportBuilder::default().with_inner_size(Vec2 { x: 600.0, y: 500.0 }),
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
    manager: Result<wl_tablet::Manager, wl_tablet::builder::BuildError>,
}
impl Viewer {
    fn new(context: &CreationContext<'_>) -> Self {
        // Context gives us access to the handle, connect to the tablet server:
        Self {
            // Safety: Destroyed in `on_exit`, before we lose the display.
            manager: unsafe {
                wl_tablet::Builder::new().build_raw(context.display_handle().unwrap().as_raw())
            },
        }
    }
}
impl eframe::App for Viewer {
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        // Drop the tablet, since our connection to the server is soon over.
        // Replace with dummy err.
        self.manager = Err(wl_tablet::builder::BuildError::Unsupported);
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
        // FIXME: only do when interacting!
        let has_tools = !manager.tools().is_empty();
        let summary = manager.pump().unwrap().summarize();

        // If an interaction is ongoing, request redraws often.
        if matches!(summary.tool, wl_tablet::events::summary::ToolState::In(_)) {
            // Arbitrary time that should be fast enough for even the fanciest of monitors x3
            ctx.request_repaint_after(std::time::Duration::from_secs_f32(1.0 / 144.0));
        } else {
            // poll for new events. (Egui will not necessarily notice the tablet input and so won't repaint on its own!)
            ctx.request_repaint_after(std::time::Duration::from_millis(250));
        }

        // Show an area to test various axes
        egui::TopBottomPanel::bottom("viewer")
            .exact_height(ctx.available_rect().height() / 2.0)
            .frame(Frame::canvas(&ctx.style()))
            .show_animated(ctx, has_tools, |ui| ui.add(ShowPen { summary }));

        // Show an info panel listing connected devices and their capabilities
        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical()
                .auto_shrink(false)
                .show(ui, |ui| {
                    ui.label(RichText::new("wl-tablet viewer ~ Connected :3").heading());
                    ui.separator();

                    ui.label("Tablets");
                    for (idx, tablet) in manager.tablets().iter().enumerate() {
                        egui::CollapsingHeader::new(&tablet.name)
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
                            ui.label(format!("Buttons: {}", pad.button_count));
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
                        let name = if let Some(id) = tool.id {
                            format!("{type_name} (ID: {id:08X})")
                        } else {
                            format!("{type_name} (ID Unknown)")
                        };
                        egui::CollapsingHeader::new(name)
                            .id_source((tool.id, tool.wacom_id, idx))
                            .show(ui, |ui| {
                                ui.label(format!("Wacom ID: {:08X?}", tool.wacom_id,));
                                for axis in tool.available_axes.iter_axes() {
                                    ui.label(format!(
                                        "{} - {:?}",
                                        axis.as_ref(),
                                        tool.axis(axis).unwrap()
                                    ));
                                    if axis == Axis::Distance {
                                        ui.label(format!(
                                            "    Distance units: {:?}",
                                            tool.distance_unit().unwrap()
                                        ));
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

/// Pen test area, showing off collected event data visually.
struct ShowPen<'a> {
    summary: Summary<'a>,
}
impl egui::Widget for ShowPen<'_> {
    fn ui(self, ui: &mut egui::Ui) -> egui::Response {
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
        if let wl_tablet::events::summary::ToolState::In(state) = self.summary.tool {
            // ======= Interation text ======
            {
                // Show the name
                let mut cursor = resp.rect.left_top();
                let pen_name = match (state.tool.id, state.tool.tool_type) {
                    (None, None) => "Unknown Tool".to_owned(),
                    (Some(id), None) => format!("{:08X}", id),
                    (None, Some(ty)) => ty.as_ref().to_string(),
                    (Some(id), Some(ty)) => format!("{} {:08X}", ty.as_ref(), id),
                };
                let rect = painter.text(
                    cursor,
                    Align2::LEFT_TOP,
                    format!("{pen_name} on {}", state.tablet.name),
                    FontId::default(),
                    Color32::WHITE,
                );
                cursor.y += rect.height();
                // I can't visualize tools I can't test :V
                let visualized_axes =
                    AvailableAxes::PRESSURE | AvailableAxes::TILT | AvailableAxes::DISTANCE;
                let seen_axes = state.tool.available_axes.intersection(visualized_axes);
                let not_seen_axes = state.tool.available_axes.difference(visualized_axes);
                let axes = if !not_seen_axes.is_empty() {
                    // Pen supports axes not visualized.
                    format!("Showing axes: {seen_axes:?}. No visualizers for {not_seen_axes:?}!")
                } else {
                    // We show all axes! yay!
                    format!("Showing all axes: {seen_axes:?}")
                };
                // Show the capabilities
                painter.text(
                    cursor,
                    Align2::LEFT_TOP,
                    axes,
                    FontId::default(),
                    Color32::WHITE,
                );
            }
            // ======= Visualize Pose ======
            ui.ctx().set_cursor_icon(egui::CursorIcon::None);
            // Fade out with distance.
            let opacity = 1.0
                - state
                    .pose
                    .distance
                    .get()
                    .map(|v| v.powf(1.5))
                    .unwrap_or(0.0);

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
