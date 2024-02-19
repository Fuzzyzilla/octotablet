use super::{Pose, TouchSource, ID};

pub struct Tool {
    pub id: ID,
    pub tablet_id: ID,
    pub down: bool,
    pub pose: Pose,
    // Names of the currently held buttons.
    pub buttons: smallvec::SmallVec<[u32; 4]>,
}
#[derive(Default, Copy, Clone)]
pub struct PadButton {
    pub pressed: bool,
    pub count: usize,
}
#[derive(Clone, Copy, Debug)]
pub struct PressInfo {
    pub source: TouchSource,
    /// Newly pressed, set when posed and unset when frame.
    pub just_pressed: bool,
    /// Not all hardware reports end events. Thus, we expire after a time of inaction.
    pub expiry: std::time::Instant,
}
impl PressInfo {
    const EXPIRY_DURATION: std::time::Duration = std::time::Duration::from_secs(1);
    pub fn new(source: TouchSource) -> Self {
        PressInfo {
            source,
            just_pressed: true,
            expiry: std::time::Instant::now() + Self::EXPIRY_DURATION,
        }
    }
    pub fn refresh_expiry(&mut self) {
        self.expiry = std::time::Instant::now() + Self::EXPIRY_DURATION;
    }
}
pub struct Strip {
    pub id: ID,
    pub last_pos: Option<f32>,
    pub delta: Option<f32>,
    pub pressed: Option<PressInfo>,
}
impl Strip {
    pub fn set(&mut self, new_pos: f32) {
        if new_pos.is_nan() {
            return;
        }
        // If pressed *and* not just pressed this frame
        if self.pressed.is_some_and(|pressed| !pressed.just_pressed) {
            // Was pressed when the update happened, update delta for the sliding motion.
            if let Some(last_pos) = self.last_pos {
                let delta = self.delta.get_or_insert(0.0);
                *delta += new_pos - last_pos;
            }
        }
        self.last_pos = Some(new_pos);
    }
    pub fn frame(&mut self) {
        if let Some(press) = self.pressed.as_mut() {
            press.just_pressed = false;
            press.refresh_expiry();
        }
    }
}
pub struct Ring {
    pub id: ID,
    pub last_pos: Option<f32>,
    pub delta: Option<f32>,
    pub pressed: Option<PressInfo>,
}
impl Ring {
    pub fn set(&mut self, new_pos: f32) {
        if new_pos.is_nan() {
            return;
        }
        // If pressed *and* not just pressed this frame
        if self.pressed.is_some_and(|pressed| !pressed.just_pressed) {
            // Was pressed when the update happened, update delta for the sliding motion.
            if let Some(last_pos) = self.last_pos {
                let delta = self.delta.get_or_insert(0.0);
                let rotations = [
                    last_pos - std::f32::consts::TAU,
                    last_pos,
                    last_pos + std::f32::consts::TAU,
                ];
                // Find variation with smallest absolute delta.
                // This is to handle the continuity break from jumping from the end of one rotation
                // to the start of the next.
                let nearest_delta = rotations
                    .iter()
                    .map(|rot| new_pos - rot)
                    // total_cmp ok, always non-nan.
                    .min_by(|&a, &b| (a.abs()).total_cmp(&b.abs()));
                // Always some, this array is non-empty
                *delta += nearest_delta.unwrap();
            }
        }
        self.last_pos = Some(new_pos);
    }
    pub fn frame(&mut self) {
        if let Some(press) = self.pressed.as_mut() {
            press.just_pressed = false;
            press.refresh_expiry();
        }
    }
}
pub struct Pad {
    pub id: ID,
    pub tablet_id: Option<ID>,
    pub buttons: Vec<PadButton>,
    pub groups: Vec<Group>,
}
pub struct Group {
    pub id: ID,
    pub mode: Option<u32>,
    pub rings: Vec<Ring>,
    pub strips: Vec<Strip>,
}
#[derive(Default)]
pub struct Summary {
    pub tool: Option<Tool>,
    pub pads: Vec<Pad>,
}
impl Summary {
    /// Get the tool summary for this tool. If None, don't summarize this tool.
    /// Doesn't automagically insert. Must be done manually on "In" (as we'd need it's associated tablet)
    pub fn tool_mut(&mut self, tool_id: &ID) -> Option<&mut Tool> {
        let tool = self.tool.as_mut()?;
        // There's a tool summary already. Make sure it's for this tool, otherwise none.
        (&tool.id == tool_id).then_some(tool)
    }
    pub fn pad_or_insert_mut(&mut self, pad: ID) -> &mut Pad {
        if let Some(pos) = self.pads.iter().position(|p| p.id == pad) {
            &mut self.pads[pos]
        } else {
            self.pads.push(Pad {
                id: pad,
                tablet_id: None,
                buttons: Vec::new(),
                groups: Vec::new(),
            });
            self.pads.last_mut().unwrap()
        }
    }
    // Lack of typesafety on these ID types makes me squirm..
    pub fn group_or_insert_mut(&mut self, pad: ID, group: ID) -> &mut Group {
        let pad = self.pad_or_insert_mut(pad);
        if let Some(pos) = pad.groups.iter().position(|g| g.id == group) {
            &mut pad.groups[pos]
        } else {
            pad.groups.push(Group {
                id: group,
                mode: None,
                rings: Vec::new(),
                strips: Vec::new(),
            });
            pad.groups.last_mut().unwrap()
        }
    }
    pub fn ring_or_insert_mut(&mut self, pad: ID, group: ID, ring: ID) -> &mut Ring {
        let group = self.group_or_insert_mut(pad, group);
        if let Some(pos) = group.rings.iter().position(|r| r.id == ring) {
            &mut group.rings[pos]
        } else {
            group.rings.push(Ring {
                id: ring,
                last_pos: None,
                delta: None,
                pressed: None,
            });
            group.rings.last_mut().unwrap()
        }
    }
    pub fn strip_or_insert_mut(&mut self, pad: ID, group: ID, strip: ID) -> &mut Strip {
        let group = self.group_or_insert_mut(pad, group);
        if let Some(pos) = group.strips.iter().position(|r| r.id == strip) {
            &mut group.strips[pos]
        } else {
            group.strips.push(Strip {
                id: strip,
                last_pos: None,
                delta: None,
                pressed: None,
            });
            group.strips.last_mut().unwrap()
        }
    }
    #[allow(clippy::too_many_lines)]
    pub fn make_concrete<'m, 's: 'm>(
        &'s self,
        manager: &'m super::Manager,
    ) -> crate::events::summary::Summary<'m> {
        use crate::events::summary;
        use crate::PlatformImpl;
        let try_summarize_tool = || -> Option<summary::InState> {
            let tool_summary = self.tool.as_ref()?;

            let tablet = manager
                .tablets()
                .iter()
                .find(|tab| tab.internal_id.unwrap_wl() == &tool_summary.tablet_id)?;
            let tool = manager
                .tools()
                .iter()
                .find(|tab| tab.internal_id.unwrap_wl() == &tool_summary.id)?;
            Some(summary::InState {
                tablet,
                tool,
                pose: tool_summary.pose,
                down: tool_summary.down,
                pressed_buttons: &tool_summary.buttons,
            })
        };

        summary::Summary {
            tool: try_summarize_tool()
                .map(summary::ToolState::In)
                .unwrap_or_default(),
            pads: self
                .pads
                .iter()
                .filter_map(|raw_pad_summary| {
                    // Bail if not found.
                    let pad = manager
                        .pads()
                        .iter()
                        .find(|pad| pad.internal_id == raw_pad_summary.id.clone().into())?;

                    Some(summary::PadState {
                        pad,
                        // Fail back to None if not found.
                        tablet: raw_pad_summary.tablet_id.clone().and_then(|tab_id| {
                            manager
                                .tablets()
                                .iter()
                                .find(|tab| tab.internal_id == tab_id.clone().into())
                        }),
                        groups: raw_pad_summary
                            .groups
                            .iter()
                            .filter_map(|raw_group_summary| {
                                let group = pad.groups.iter().find(|g| {
                                    g.internal_id == raw_group_summary.id.clone().into()
                                })?;
                                Some(summary::GroupState {
                                    group,
                                    mode: raw_group_summary.mode,
                                    rings: raw_group_summary
                                        .rings
                                        .iter()
                                        .filter_map(|raw_ring_summary| {
                                            let ring = group.rings.iter().find(|r| {
                                                r.internal_id == raw_ring_summary.id.clone().into()
                                            })?;
                                            Some(summary::RingState {
                                                ring,
                                                angle: raw_ring_summary.last_pos,
                                                touched_by: raw_ring_summary
                                                    .pressed
                                                    .map(|p| p.source),
                                                delta_radians: raw_ring_summary.delta,
                                            })
                                        })
                                        .collect(),
                                    strips: raw_group_summary
                                        .strips
                                        .iter()
                                        .filter_map(|raw_strip_summary| {
                                            let strip = group.strips.iter().find(|s| {
                                                s.internal_id == raw_strip_summary.id.clone().into()
                                            })?;
                                            Some(summary::StripState {
                                                strip,
                                                position: raw_strip_summary.last_pos,
                                                delta: raw_strip_summary.delta,
                                                touched_by: raw_strip_summary
                                                    .pressed
                                                    .map(|p| p.source),
                                            })
                                        })
                                        .collect(),
                                })
                            })
                            .collect(),
                        buttons: raw_pad_summary
                            .buttons
                            .iter()
                            .enumerate()
                            .map(|(button_idx, raw_button)| {
                                let button_idx = u32::try_from(button_idx).unwrap();
                                summary::PadButtonState {
                                    // Find the owner of this button's index, if any.
                                    group: pad.groups.iter().find(|group| {
                                        // Sorted. Will be Ok if binary search finds it.
                                        group.buttons.binary_search(&button_idx).is_ok()
                                    }),
                                    currently_pressed: raw_button.pressed,
                                    count: raw_button.count,
                                }
                            })
                            .collect(),
                    })
                })
                .collect(),
        }
    }
    /// Clear all one-shot properties, such as per-frame button counts.
    pub fn consume_oneshot(&mut self) {
        let now = std::time::Instant::now();
        for pad in &mut self.pads {
            for button in &mut pad.buttons {
                button.count = 0;
            }
            for group in &mut pad.groups {
                for ring in &mut group.rings {
                    ring.delta = None;
                    // Destroy pressed event if expired
                    if ring
                        .pressed
                        .as_ref()
                        .is_some_and(|pressed| now > pressed.expiry)
                    {
                        ring.pressed = None;
                    }
                }
                for strip in &mut group.strips {
                    strip.delta = None;
                    // Destroy pressed event if expired
                    if strip
                        .pressed
                        .as_ref()
                        .is_some_and(|pressed| now > pressed.expiry)
                    {
                        strip.pressed = None;
                    }
                }
            }
        }
    }
}
