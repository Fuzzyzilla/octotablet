//! Dispatch impls for pad-related events

use super::{
    raw_events, summary::PadButton, summary::PressInfo, wl_tablet, Connection, Dispatch,
    FrameTimestamp, Group, HasWlId, Proxy, QueueHandle, Ring, Strip, TabletState, TouchSource,
};

impl Dispatch<wl_tablet::zwp_tablet_pad_v2::ZwpTabletPadV2, ()> for TabletState {
    fn event(
        this: &mut Self,
        pad: &wl_tablet::zwp_tablet_pad_v2::ZwpTabletPadV2,
        event: wl_tablet::zwp_tablet_pad_v2::Event,
        (): &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        use wl_tablet::zwp_tablet_pad_v2::Event;
        #[allow(clippy::match_same_arms)]
        match event {
            // ======= Constructor databurst =========
            Event::Group { pad_group } => {
                let ctor = this.partial_pads.get_or_insert_ctor(pad.id());
                ctor.groups.push(Group::new_default(pad_group.id()));
                // Remember that this group id is associated with this pad.
                this.group_associations.insert(pad_group.id(), pad.id());
            }
            Event::Path { .. } => (),
            Event::Buttons { buttons } => {
                let ctor = this.partial_pads.get_or_insert_ctor(pad.id());
                ctor.total_buttons = buttons;
            }
            Event::Done => {
                let pad_id = pad.id();
                if let Some(Ok(pad)) = this.partial_pads.done(&pad_id) {
                    this.pads.push(pad);
                    this.events.push(raw_events::Event::Pad {
                        pad: pad_id,
                        event: raw_events::PadEvent::Added,
                    });
                }
            }
            Event::Removed => {
                this.raw_summary.pads.retain(|p| p.id != pad.id());
                this.destroy_pad(pad.id());
                this.events.push(raw_events::Event::Pad {
                    pad: pad.id(),
                    event: raw_events::PadEvent::Removed,
                });
            }
            // ======== Interaction data =========
            Event::Button {
                button,
                state,
                time, //use me!
            } => {
                let pressed = matches!(
                    state,
                    wayland_client::WEnum::Value(
                        wl_tablet::zwp_tablet_pad_v2::ButtonState::Pressed
                    )
                );
                // Update summary
                {
                    let pad_summary = this.raw_summary.pad_or_insert_mut(pad.id());
                    // Ensure there are enough button elements.
                    if pad_summary.buttons.len() <= button as usize {
                        // Button is a zero-based index, so add one
                        pad_summary
                            .buttons
                            .resize(button as usize + 1, PadButton::default());
                    }
                    // Increase count if going from unpressed->pressed
                    let button = &mut pad_summary.buttons[button as usize];
                    if !button.pressed && pressed {
                        button.count = button.count.saturating_add(1);
                    }
                    button.pressed = pressed;
                }
                // Send event
                this.events.push(raw_events::Event::Pad {
                    pad: pad.id(),
                    event: raw_events::PadEvent::Button {
                        button_idx: button,
                        pressed,
                    },
                });
            }
            Event::Enter { tablet, .. } => {
                this.raw_summary.pad_or_insert_mut(pad.id()).tablet_id = Some(tablet.id());
                this.events.push(raw_events::Event::Pad {
                    pad: pad.id(),
                    event: raw_events::PadEvent::Enter {
                        tablet: tablet.id(),
                    },
                });
            }
            Event::Leave { .. } => {
                this.raw_summary.pad_or_insert_mut(pad.id()).tablet_id = None;
                this.events.push(raw_events::Event::Pad {
                    pad: pad.id(),
                    event: raw_events::PadEvent::Exit,
                });
            }
            // ne
            _ => (),
        }
    }
    wayland_client::event_created_child!(
        TabletState,
        wl_tablet::zwp_tablet_pad_v2::ZwpTabletPadV2,
        [
            wl_tablet::zwp_tablet_pad_v2::EVT_GROUP_OPCODE => (wl_tablet::zwp_tablet_pad_group_v2::ZwpTabletPadGroupV2, ()),
        ]
    );
}
impl Dispatch<wl_tablet::zwp_tablet_pad_group_v2::ZwpTabletPadGroupV2, ()> for TabletState {
    fn event(
        this: &mut Self,
        group: &wl_tablet::zwp_tablet_pad_group_v2::ZwpTabletPadGroupV2,
        event: wl_tablet::zwp_tablet_pad_group_v2::Event,
        (): &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        // Everything (aside from the ctor databurst) needs this. Hoist it out for less code duplication...
        let pad_id = this.group_associations.get(&group.id()).cloned();
        #[allow(clippy::match_same_arms)]
        match event {
            // ======= Constructor databurst =========
            wl_tablet::zwp_tablet_pad_group_v2::Event::Buttons { buttons } => {
                // Buttons *seems* to be a byte array, where each chunk of 4 is a `u32` in native endian order,
                // listing the indices of the global pad buttons that are uniquely owned by this group.
                // This is all just my best guess! Ahh!
                // Truncates to four byte chunks. That seems like a server error if my interpretation of the arcane
                // values are correct.
                let to_u32 = buttons.chunks_exact(4).map(|bytes| {
                    let Ok(bytes): Result<[u8; 4], _> = bytes.try_into() else {
                        // Guaranteed by chunks exact but not shown at a type-level.
                        unreachable!()
                    };
                    u32::from_ne_bytes(bytes)
                });
                let ctor = this.partial_groups.get_or_insert_ctor(group.id());
                ctor.buttons.extend(to_u32);
                // Make some adjustments to be slightly more reasonable to use x3
                // These collections will be trivially tiny so this is fine to do.
                ctor.buttons.sort_unstable();
                ctor.buttons.dedup();
            }
            wl_tablet::zwp_tablet_pad_group_v2::Event::Modes { modes } => {
                // This event only sent when modes > 0.
                let ctor = this.partial_groups.get_or_insert_ctor(group.id());
                // Will always be Some(modes), but panicless in the case of a server impl bug.
                ctor.mode_count = std::num::NonZeroU32::new(modes);
            }
            wl_tablet::zwp_tablet_pad_group_v2::Event::Ring { ring } => {
                this.ring_associations.insert(ring.id(), group.id());
                let ctor = this.partial_groups.get_or_insert_ctor(group.id());
                ctor.rings.push(Ring {
                    granularity: None,
                    internal_id: ring.id().into(),
                });
            }
            wl_tablet::zwp_tablet_pad_group_v2::Event::Strip { strip } => {
                this.strip_associations.insert(strip.id(), group.id());
                let ctor = this.partial_groups.get_or_insert_ctor(group.id());
                ctor.strips.push(Strip {
                    granularity: None,
                    internal_id: strip.id().into(),
                });
            }
            wl_tablet::zwp_tablet_pad_group_v2::Event::Done => {
                // Finish the group and add it the associated pad.
                // *Confused screaming*
                let group_id = group.id();
                if let Some(Ok(group)) = this.partial_groups.done(&group_id) {
                    if let Some(pad_id) = pad_id {
                        // Pad may be finished already or still in construction.
                        let pad = if let Some(pad) =
                            this.pads.iter_mut().find(|p| HasWlId::id(*p) == &pad_id)
                        {
                            pad
                        } else {
                            this.partial_pads.get_or_insert_ctor(pad_id)
                        };
                        // Replace existing group of this id, or add new.
                        // This is all weird hacky ctor ordering nonsense...
                        if let Some(pos) =
                            pad.groups.iter().position(|g| HasWlId::id(g) == &group_id)
                        {
                            pad.groups[pos] = group;
                        } else {
                            pad.groups.push(group);
                        }
                    }
                }
            }
            // ======== Interaction data =========
            wl_tablet::zwp_tablet_pad_group_v2::Event::ModeSwitch {
                mode,
                time: _, //use me!
                ..
            } => {
                let Some(pad_id) = pad_id else { return };
                this.raw_summary
                    .group_or_insert_mut(pad_id.clone(), group.id())
                    .mode = Some(mode);
                this.events.push(raw_events::Event::Pad {
                    pad: pad_id,
                    event: raw_events::PadEvent::Group {
                        group: group.id(),
                        event: raw_events::PadGroupEvent::Mode(mode),
                    },
                });
            }
            // ne
            _ => (),
        }
    }
    wayland_client::event_created_child!(
        TabletState,
        wl_tablet::zwp_tablet_pad_group_v2::ZwpTabletPadGroupV2,
        [
            wl_tablet::zwp_tablet_pad_group_v2::EVT_RING_OPCODE => (wl_tablet::zwp_tablet_pad_ring_v2::ZwpTabletPadRingV2, ()),
            wl_tablet::zwp_tablet_pad_group_v2::EVT_STRIP_OPCODE => (wl_tablet::zwp_tablet_pad_strip_v2::ZwpTabletPadStripV2, ()),
        ]
    );
}
impl Dispatch<wl_tablet::zwp_tablet_pad_ring_v2::ZwpTabletPadRingV2, ()> for TabletState {
    fn event(
        this: &mut Self,
        ring: &wl_tablet::zwp_tablet_pad_ring_v2::ZwpTabletPadRingV2,
        event: wl_tablet::zwp_tablet_pad_ring_v2::Event,
        (): &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let Some(group) = this.ring_associations.get(&ring.id()).cloned() else {
            return;
        };
        let Some(pad) = this.group_associations.get(&group).cloned() else {
            return;
        };
        #[allow(clippy::match_same_arms)]
        match event {
            #[allow(clippy::cast_possible_truncation)]
            wl_tablet::zwp_tablet_pad_ring_v2::Event::Angle { degrees } => {
                if degrees.is_nan() {
                    return;
                }
                let degrees = degrees as f32;
                let radians = degrees.to_radians();
                this.raw_summary
                    .ring_or_insert_mut(pad.clone(), group.clone(), ring.id())
                    .set(radians);
                this.events.push(raw_events::Event::Pad {
                    pad,
                    event: raw_events::PadEvent::Group {
                        group,
                        event: raw_events::PadGroupEvent::Ring {
                            ring: ring.id(),
                            event: crate::events::TouchStripEvent::Pose(radians),
                        },
                    },
                });
            }
            wl_tablet::zwp_tablet_pad_ring_v2::Event::Source { source } => {
                // Convert source, falling back to unknown.
                let source = match source {
                    wayland_client::WEnum::Value(
                        wl_tablet::zwp_tablet_pad_ring_v2::Source::Finger,
                    ) => TouchSource::Finger,
                    _ => TouchSource::Unknown,
                };
                // Set press source and refresh if already pressed, or insert anew.
                let sum =
                    this.raw_summary
                        .ring_or_insert_mut(pad.clone(), group.clone(), ring.id());
                if let Some(press) = sum.pressed.as_mut() {
                    press.source = source;
                    press.refresh_expiry();
                } else {
                    sum.pressed = Some(PressInfo::new(source));
                }

                this.events.push(raw_events::Event::Pad {
                    pad,
                    event: raw_events::PadEvent::Group {
                        group,
                        event: raw_events::PadGroupEvent::Ring {
                            ring: ring.id(),
                            event: crate::events::TouchStripEvent::Source(source),
                        },
                    },
                });
            }
            wl_tablet::zwp_tablet_pad_ring_v2::Event::Stop => {
                this.raw_summary
                    .ring_or_insert_mut(pad.clone(), group.clone(), ring.id())
                    .pressed = None;
                this.events.push(raw_events::Event::Pad {
                    pad,
                    event: raw_events::PadEvent::Group {
                        group,
                        event: raw_events::PadGroupEvent::Ring {
                            ring: ring.id(),
                            event: crate::events::TouchStripEvent::Up,
                        },
                    },
                });
            }
            wl_tablet::zwp_tablet_pad_ring_v2::Event::Frame { time } => {
                this.raw_summary
                    .ring_or_insert_mut(pad.clone(), group.clone(), ring.id())
                    .frame();
                this.events.push(raw_events::Event::Pad {
                    pad,
                    event: raw_events::PadEvent::Group {
                        group,
                        event: raw_events::PadGroupEvent::Ring {
                            ring: ring.id(),
                            event: crate::events::TouchStripEvent::Frame(Some(FrameTimestamp(
                                std::time::Duration::from_millis(u64::from(time)),
                            ))),
                        },
                    },
                });
            }
            // ne
            _ => (),
        }
    }
}
impl Dispatch<wl_tablet::zwp_tablet_pad_strip_v2::ZwpTabletPadStripV2, ()> for TabletState {
    fn event(
        this: &mut Self,
        strip: &wl_tablet::zwp_tablet_pad_strip_v2::ZwpTabletPadStripV2,
        event: wl_tablet::zwp_tablet_pad_strip_v2::Event,
        (): &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        // BIIGGGG code duplication with Ring, i don't know how to fix that because this all comes from different modules and thus
        // is actually different types......
        let Some(group) = this.strip_associations.get(&strip.id()).cloned() else {
            return;
        };
        let Some(pad) = this.strip_associations.get(&group).cloned() else {
            return;
        };
        #[allow(clippy::match_same_arms)]
        match event {
            #[allow(clippy::cast_possible_truncation)]
            wl_tablet::zwp_tablet_pad_strip_v2::Event::Position { position } => {
                // Saturating-as (guaranteed by the protocol spec to be 0..=65535)
                let position = u16::try_from(position).unwrap_or(65535);
                let position = f32::from(position) / 65535.0;
                this.raw_summary
                    .strip_or_insert_mut(pad.clone(), group.clone(), strip.id())
                    .set(position);
                this.events.push(raw_events::Event::Pad {
                    pad,
                    event: raw_events::PadEvent::Group {
                        group,
                        event: raw_events::PadGroupEvent::Strip {
                            strip: strip.id(),
                            event: crate::events::TouchStripEvent::Pose(position),
                        },
                    },
                });
            }
            wl_tablet::zwp_tablet_pad_strip_v2::Event::Source { source } => {
                // Convert source, falling back to unknown.
                let source = match source {
                    wayland_client::WEnum::Value(
                        wl_tablet::zwp_tablet_pad_strip_v2::Source::Finger,
                    ) => TouchSource::Finger,
                    _ => TouchSource::Unknown,
                };
                // Set press source and refresh if already pressed, or insert anew.
                let sum =
                    this.raw_summary
                        .strip_or_insert_mut(pad.clone(), group.clone(), strip.id());
                if let Some(press) = sum.pressed.as_mut() {
                    press.source = source;
                    press.refresh_expiry();
                } else {
                    sum.pressed = Some(PressInfo::new(source));
                }
                this.events.push(raw_events::Event::Pad {
                    pad,
                    event: raw_events::PadEvent::Group {
                        group,
                        event: raw_events::PadGroupEvent::Strip {
                            strip: strip.id(),
                            event: crate::events::TouchStripEvent::Source(source),
                        },
                    },
                });
            }
            wl_tablet::zwp_tablet_pad_strip_v2::Event::Stop => {
                this.raw_summary
                    .strip_or_insert_mut(pad.clone(), group.clone(), strip.id())
                    .pressed = None;
                this.events.push(raw_events::Event::Pad {
                    pad,
                    event: raw_events::PadEvent::Group {
                        group,
                        event: raw_events::PadGroupEvent::Strip {
                            strip: strip.id(),
                            event: crate::events::TouchStripEvent::Up,
                        },
                    },
                });
            }
            wl_tablet::zwp_tablet_pad_strip_v2::Event::Frame { time } => {
                this.raw_summary
                    .strip_or_insert_mut(pad.clone(), group.clone(), strip.id())
                    .frame();
                this.events.push(raw_events::Event::Pad {
                    pad,
                    event: raw_events::PadEvent::Group {
                        group,
                        event: raw_events::PadGroupEvent::Strip {
                            strip: strip.id(),
                            event: crate::events::TouchStripEvent::Frame(Some(FrameTimestamp(
                                std::time::Duration::from_millis(u64::from(time)),
                            ))),
                        },
                    },
                });
            }
            // ne
            _ => (),
        }
    }
}
