//! Dispatch impls for Tool-related events.
use super::{
    raw_events, wl_tablet, Connection, Dispatch, FrameState, Proxy, QueueHandle, TabletState,
};

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
                use crate::axis;
                use wl_tablet::zwp_tablet_tool_v2::Capability;
                let ctor = this.partial_tools.get_or_insert_ctor(tool.id());

                // The wayland protocol makes blanket guarantees about the precise aspects of the ranges
                // and units of these axes, they are not dynamic per-device. We don't report the "65535" granularities
                // since they would be highly misleading to the user - we don't know that the device *actually* reports
                // that finely (unlikely).
                match capability {
                    Capability::Distance => {
                        ctor.axes.distance = Some(axis::LengthInfo::default());
                    }
                    Capability::Pressure => {
                        ctor.axes.pressure = Some(axis::NormalizedInfo::default());
                    }
                    Capability::Rotation => ctor.axes.roll = Some(axis::CircularInfo::default()),
                    Capability::Slider => {
                        ctor.axes.slider = Some(axis::SliderInfo::default());
                    }
                    Capability::Tilt => {
                        ctor.axes.tilt = Some(axis::Info {
                            limits: Some((-std::f32::consts::PI..=std::f32::consts::PI).into()),
                            granularity: None,
                        });
                    }
                    Capability::Wheel => {
                        ctor.axes.wheel = Some(axis::CircularInfo::default());
                    }
                    // ne
                    _ => (),
                }
            }
            Event::HardwareIdWacom {
                hardware_id_hi,
                hardware_id_lo,
            } => {
                let ctor = this.partial_tools.get_or_insert_ctor(tool.id());
                ctor.wacom_id = Some(u64::from(hardware_id_hi) << 32 | u64::from(hardware_id_lo));
            }
            Event::HardwareSerial {
                hardware_serial_hi,
                hardware_serial_lo,
            } => {
                let ctor = this.partial_tools.get_or_insert_ctor(tool.id());
                ctor.hardware_id = Some(crate::tool::HardwareID(
                    u64::from(hardware_serial_hi) << 32 | u64::from(hardware_serial_lo),
                ));
            }
            Event::Type {
                tool_type: wayland_client::WEnum::Value(tool_type),
            } => {
                use crate::tool::Type;
                use wl_tablet::zwp_tablet_tool_v2::Type as WlType;
                let ctor = this.partial_tools.get_or_insert_ctor(tool.id());
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
                this.events.push(raw_events::Event::Tool {
                    tool: tool.id(),
                    event: raw_events::ToolEvent::Added,
                });
                if let Some(Ok(tool)) = this.partial_tools.done(&tool.id()) {
                    this.tools.push(tool);
                }
            }
            Event::Removed => {
                this.destroy_tool(tool.id());
                this.events.push(raw_events::Event::Tool {
                    tool: tool.id(),
                    event: raw_events::ToolEvent::Removed,
                });
            }
            // ======== Interaction data =========
            Event::ProximityIn { tablet, .. } => {
                this.frame_in_progress(tool.id()).state_transition =
                    Some(FrameState::In(tablet.id()));
            }
            Event::ProximityOut { .. } => {
                this.frame_in_progress(tool.id()).state_transition = Some(FrameState::Out);
            }
            Event::Down { .. } => {
                this.frame_in_progress(tool.id()).state_transition = Some(FrameState::Down);
            }
            Event::Up { .. } => {
                this.frame_in_progress(tool.id()).state_transition = Some(FrameState::Up);
            }
            #[allow(clippy::cast_possible_truncation)]
            Event::Motion { x, y } => {
                let x = x as f32;
                let y = y as f32;
                this.frame_in_progress(tool.id()).position = Some([x, y]);
            }
            #[allow(clippy::cast_possible_truncation)]
            Event::Tilt { tilt_x, tilt_y } => {
                let tilt_x = (tilt_x as f32).to_radians();
                let tilt_y = (tilt_y as f32).to_radians();
                this.frame_in_progress(tool.id()).tilt = Some([tilt_x, tilt_y]);
            }
            Event::Pressure { pressure } => {
                // Saturating-as (guaranteed by the protocol spec to be 0..=65535)
                let pressure = u16::try_from(pressure).unwrap_or(65535);
                let pressure = f32::from(pressure) / 65535.0;
                this.frame_in_progress(tool.id()).pressure = Some(pressure);
            }
            Event::Distance { distance } => {
                // Saturating-as (guaranteed by the protocol spec to be 0..=65535)
                let distance = u16::try_from(distance).unwrap_or(65535);
                let distance = f32::from(distance) / 65535.0;
                this.frame_in_progress(tool.id()).distance = Some(distance);
            }
            #[allow(clippy::cast_possible_truncation)]
            Event::Rotation { degrees } => {
                let radians = (degrees as f32).to_radians();
                this.frame_in_progress(tool.id()).roll = Some(radians);
            }
            Event::Slider { position } => {
                // Saturating-as (guaranteed by the protocol spec to be 0..=65535)
                let position = u16::try_from(position).unwrap_or(65535);
                let position = f32::from(position) / 65535.0;
                this.frame_in_progress(tool.id()).slider = Some(position);
            }
            Event::Button { button, state, .. } => {
                let pressed = matches!(
                    state,
                    wayland_client::WEnum::Value(
                        wl_tablet::zwp_tablet_tool_v2::ButtonState::Pressed
                    )
                );
                this.frame_in_progress(tool.id())
                    .buttons
                    .push((button, pressed));
            }
            Event::Frame { time } => {
                this.frame(&tool.id(), time);
            }

            // ne
            _ => (),
        }
    }
}
