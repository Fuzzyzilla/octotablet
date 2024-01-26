use eframe::{
    egui::{self, RichText},
    CreationContext, EventLoopBuilder,
};
use raw_window_handle::HasRawDisplayHandle;
use winit::platform::wayland::EventLoopBuilderExtWayland;
use wl_tablet::{tool::Axis, Manager};

fn main() {
    // Dont persist, require wayland.
    let native_options = eframe::NativeOptions {
        persist_window: false,
        event_loop_builder: Some(Box::new(|event_loop: &mut EventLoopBuilder<_>| {
            event_loop.with_wayland();
        })),
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

struct Viewer {
    manager: Result<wl_tablet::Manager, wl_tablet::ManagerError>,
}
impl Viewer {
    fn new(context: &CreationContext<'_>) -> Self {
        // Context gives us access to the handle, connect to the tablet server:
        Self {
            // Safety: Destroyed in `on_exit`, before we lose the display.
            manager: unsafe { Manager::new_raw(context.raw_display_handle()) },
        }
    }
}
impl eframe::App for Viewer {
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        // Drop the tablet, since our connection to the server is soon over.
        // Replace with dummy err.
        self.manager = Err(wl_tablet::ManagerError::Unsupported);
    }
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            let manager = match &mut self.manager {
                Ok(t) => t,
                Err(e) => {
                    ui.label(
                        RichText::new(format!("Failed to acquire tablet: {e}"))
                            .monospace()
                            .heading(),
                    );
                    return;
                }
            };
            manager.pump().unwrap();

            ui.label(RichText::new("Hardware").heading());
            ui.separator();

            ui.label("Tablets");
            for tablet in manager.tablets() {
                egui::CollapsingHeader::new(&tablet.name).show(ui, |_| ());
            }
            ui.separator();
            ui.label("Tools");
            for (idx, tool) in manager.tools().iter().enumerate() {
                let type_name = tool.tool_type.as_ref().map_or("Unknown", |ty| ty.as_ref());
                let name = if let Some(id) = tool.id {
                    format!("{type_name} (ID: {id:X})")
                } else {
                    format!("{type_name} (ID Unknown)")
                };
                egui::CollapsingHeader::new(name)
                    .id_source((tool.id, tool.wacom_id, idx))
                    .show(ui, |ui| {
                        ui.label(format!("Wacom ID: {:?}", tool.wacom_id,));
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
            ui.separator();
        });
    }
}
