//! Implementation details for Window's Ink `RealTimeStylus` interface.
//!
//! Within this module, it is sound to assume `cfg(ink_rts) == true`
//! (compiling for a wayland target + has deps, or is building docs).

use std::sync;

use windows::core::{self, Result as WinResult};
use windows::Win32::Foundation::{E_FAIL, E_INVALIDARG, E_POINTER, HANDLE_PTR, HWND, POINT};
use windows::Win32::System::Com as com;
use windows::Win32::UI::TabletPC as tablet_pc;

use crate::axis::Union;

const HIMETRIC_PER_INCH: f32 = 2540.0;

mod packet;

/// Multiplier to get from HIMETRIC physical units to logical pixel space for the given window. This should be re-called frequently
/// to handle dynamic DPI (icky, but that's [microsoft's word](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-getdpiforsystem#remarks) not mine!)
/// # Safety
/// `hwnd` must be a valid window handle.
#[allow(clippy::cast_precision_loss)]
unsafe fn fetch_himetric_to_logical_pixel(hwnd: HWND) -> f32 {
    // The value of this call depends on the *thread's* DPI awareness registration with the system.
    // That's left to the calling thread, but this *should* always work on any of the awareness settings.

    // Safety: idk. It's not about whether `hwnd` is valid, but idk what it *is* about :P
    let mut dpi = unsafe { windows::Win32::UI::HiDpi::GetDpiForWindow(hwnd) };

    // Uh oh, failed to fetch... Try for system instead.
    if dpi == 0 {
        // Safety: lmao
        dpi = unsafe { windows::Win32::UI::HiDpi::GetDpiForSystem() };
    }

    // Rounding is ok - in practice, this is really not a large value. Will never be NaN or INF which is the real problem.
    (dpi as f32) / HIMETRIC_PER_INCH
}

#[derive(Clone, Copy, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub enum ID {
    /// "Tablet contextual ID" (`tcid`)
    Tablet(u32),
    /// IInkCursor ID
    Stylus { sid: u32, cursor_id: Option<i32> },
}

#[derive(Copy, Clone, Hash, PartialEq, Eq)]
pub struct ButtonID(core::GUID);
impl PartialOrd for ButtonID {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for ButtonID {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        let Self(core::GUID {
            data1: s1,
            data2: s2,
            data3: s3,
            data4: s4,
        }) = self;
        let Self(core::GUID {
            data1: o1,
            data2: o2,
            data3: o3,
            data4: o4,
        }) = other;

        (s1, s2, s3, s4).cmp(&(o1, o2, o3, o4))
    }
}

#[derive(Clone, Debug)]
struct RawTablet {
    interpreter: packet::Interpreter,
    // In Ink, the axis capabilities are a property of the *tablet*, not the tool.
    // To bridge this gap, this field will be virally spread to any tool that interacts with this tablet.
    axes: crate::axis::FullInfo,
    tcid: u32,
}

#[derive(Clone, Debug)]
// This is fine. They should *all* be this big and the smol one is the sadpath.
#[allow(clippy::large_enum_variant)]
enum RawTabletSlot {
    /// There is no user-facing tablet associated with this slot, but the slot is still taken.
    /// Used when tablet initialization fails (for any multitude of reasons) but our array order
    /// *must* remain in sync with the server's.
    Dummy { tcid: u32 },
    /// There is a concrete tablet made from this one with the given `tcid`.
    Concrete(RawTablet),
}
impl RawTabletSlot {
    pub fn tcid(&self) -> u32 {
        match self {
            Self::Concrete(raw) => raw.tcid,
            Self::Dummy { tcid } => *tcid,
        }
    }
}

/// The full inner state
struct DataFrame {
    /// Cached value from [`fetch_himetric_to_logical_pixel`], updated when needed.
    hwnd: HWND,
    himetric_to_logical_pixel: f32,
    /// Keep track of the current state of styluses. Whether included at all indicates In/Out state.
    stylus_states: std::collections::BTreeMap<ID, StylusPhase>,
    tools: Vec<crate::tool::Tool>,
    /// Indicies of `raw_tablets` which have been queued for destruction after events are consumed.
    /// This is the *actual* subscript into the list, *not* apparent index (index where deleted ones aren't counted)
    /// Must always be sorted and deduplicated.
    raw_tablet_deletions: Vec<usize>,
    /// Tablets *in initialization order*.
    /// This is important, since notifications may refer to a tablet by it's index.
    raw_tablets: Vec<RawTabletSlot>,
    /// User-visible tablets created from [`RawTablet::Concrete`] tablets.
    tablets: Vec<crate::tablet::Tablet>,
    events: Vec<crate::events::raw::Event<ID>>,
}
impl Clone for DataFrame {
    fn clone(&self) -> Self {
        // I hope the compiler makes this implementation less stupid :D
        let mut clone = Self {
            himetric_to_logical_pixel: 0.0,
            // uh oh. Replaced before anything icky occurs :P
            hwnd: HWND(0),
            // Empty vecs don't alloc, this is ok.
            raw_tablet_deletions: vec![],
            raw_tablets: vec![],
            stylus_states: std::collections::BTreeMap::new(),
            tools: vec![],
            tablets: vec![],
            events: vec![],
        };

        clone.clone_from(self);
        clone
    }
    fn clone_from(&mut self, source: &Self) {
        // Destructure, that way any changes to layout of self will cause a compile err.
        let Self {
            hwnd,
            himetric_to_logical_pixel,
            stylus_states,
            tools,
            raw_tablet_deletions,
            raw_tablets,
            tablets,
            events,
        } = self;
        *himetric_to_logical_pixel = source.himetric_to_logical_pixel;
        *hwnd = source.hwnd;
        stylus_states.clone_from(&source.stylus_states);

        tools.clear();
        tools.extend(
            source
                .tools
                .iter()
                // Intentionally !Clone, manually impl:
                .map(|tool| crate::tool::Tool {
                    // Clone what needs to be:
                    internal_id: tool.internal_id.clone(),
                    name: tool.name.clone(),
                    // Copy the rest:
                    ..*tool
                }),
        );

        raw_tablet_deletions.clone_from(&source.raw_tablet_deletions);
        raw_tablets.clone_from(&source.raw_tablets);

        tablets.clear();
        tablets.extend(
            source
                .tablets
                .iter()
                // Intentionally !Clone, manually impl:
                .map(|tablet| crate::tablet::Tablet {
                    // Clone what needs to be:
                    internal_id: tablet.internal_id.clone(),
                    // todo: make this clone_from, re-use the alloc!!
                    name: tablet.name.clone(),
                    // Copy the rest:
                    ..*tablet
                }),
        );

        events.clone_from(&source.events);
    }
}
impl DataFrame {
    fn tools(&self) -> &[crate::tool::Tool] {
        &self.tools
    }
    fn tablets(&self) -> &[crate::tablet::Tablet] {
        &self.tablets
    }
    fn raw_events(&self) -> &[crate::events::raw::Event<ID>] {
        &self.events
    }
    /// Destroy all internal state, even if the state is self-inconsistent.
    fn reset(&mut self) -> &mut Self {
        // Destructure, that way any changes to layout of self will cause a compile err.
        let Self {
            himetric_to_logical_pixel: _,
            hwnd: _,
            stylus_states,
            tools,
            raw_tablet_deletions,
            raw_tablets,
            tablets,
            events,
        } = self;

        stylus_states.clear();
        tools.clear();
        raw_tablet_deletions.clear();
        raw_tablets.clear();
        tablets.clear();
        events.clear();
        self
    }
    /// Called at the end of each pump to maintain bookkeeping, *after* the clone has occured
    fn frame_end_cleanup(&mut self) {
        self.events.clear();

        // Handle deletions.
        for removal in self.raw_tablet_deletions.drain(..) {
            let tcid = self.raw_tablets.remove(removal).tcid();
            // Remove the concrete tablet of the same ID. (May not exist).
            self.tablets
                .retain(|tab| *tab.internal_id.unwrap_ink() != ID::Tablet(tcid));
        }
    }
    /// From the given collection of tools, find the tool under `sid` or insert a newly populated one.
    /// (can't take a self param due to borrowing crimes.)
    fn get_or_insert_tool<'tool>(
        tools: &'tool mut Vec<crate::tool::Tool>,
        rts: &tablet_pc::IRealTimeStylus,
        sid: u32,
    ) -> WinResult<&'tool mut crate::tool::Tool> {
        if let Some(pos) = tools
            .iter()
            .position(|tool| matches!(tool.internal_id.unwrap_ink(), ID::Stylus { sid: this_sid, .. } if *this_sid == sid)) {
                // use `position` instead of find because of borrow issues.
                return Ok(&mut tools[pos]);
            };
        // Not found, make one!
        unsafe {
            // About cursors!
            // https://learn.microsoft.com/en-us/windows/win32/api/msinkaut/nn-msinkaut-iinkcursor
            // Each cursor represents *one end* of a stylus, with implications of it being hardware unique ID (note from future:
            // this implication is false). Problem is, tip and eraser have *different* hardware ids with no way to re-correlate them.
            // Sadness!
            // We can also query the number of buttons! However, tip and eraser are also considered buttons with no way
            // to differentiate them, so that's not much use.

            let cursor = rts.GetStylusForId(sid)?;
            let cursor_id = cursor.Id().ok();

            let tool = crate::tool::Tool {
                internal_id: ID::Stylus { sid, cursor_id }.into(),
                name: cursor.Name().ok().as_ref().map(ToString::to_string),
                // Very intentional cast. We just want *uniqueness* of each number.
                #[allow(clippy::cast_sign_loss)]
                hardware_id: cursor_id.map(|id| crate::tool::HardwareID(id as u64)),
                wacom_id: None,
                tool_type: match cursor
                    .Inverted()
                    .map(windows::Win32::Foundation::VARIANT_BOOL::as_bool)
                {
                    Ok(false) => Some(crate::tool::Type::Pen),
                    Ok(true) => Some(crate::tool::Type::Eraser),
                    Err(_) => None,
                },
                axes: crate::axis::FullInfo::default(),
            };
            tools.push(tool);
            Ok(tools.last_mut().unwrap())
        }
    }
    fn get_tool(&self, sid: u32) -> Option<&crate::tool::Tool> {
        self.tools
            .iter()
            .find(|tool| matches!(tool.internal_id.unwrap_ink(), ID::Stylus { sid: this_sid, .. } if *this_sid == sid))
    }
    /// Insert a tablet to the end of the raw tablets list.
    /// *Always succeeds* in appending to the list, but in the case of an error a dummy is appended.
    #[allow(clippy::needless_pass_by_value)]
    unsafe fn append_tablet(
        &mut self,
        rts: &tablet_pc::IRealTimeStylus,
        tablet: &tablet_pc::IInkTablet,
        tcid: u32,
    ) -> &mut RawTabletSlot {
        // Attempt to query how to parse a tablet's packets. if this fails, we
        // *must still make a tablet out of it*, just a dummy one!
        let (raw_tablet, tablet) =
            if let Ok((interpreter, info)) = unsafe { packet::make_interpreter(rts, tcid) } {
                (
                    RawTabletSlot::Concrete(RawTablet {
                        interpreter,
                        axes: info,
                        tcid,
                    }),
                    Some(crate::tablet::Tablet {
                        internal_id: ID::Tablet(tcid).into(),
                        name: unsafe { tablet.Name() }
                            .ok()
                            .as_ref()
                            .map(ToString::to_string),
                        usb_id: None,
                    }),
                )
            } else {
                (RawTabletSlot::Dummy { tcid }, None)
            };

        if let Some(tablet) = tablet {
            self.tablets.push(tablet);
            self.events.push(crate::events::raw::Event::Tablet {
                tablet: ID::Tablet(tcid),
                event: crate::events::raw::TabletEvent::Added,
            });
        }

        self.raw_tablets.push(raw_tablet);
        self.raw_tablets.last_mut().unwrap()
    }
    /// Delete the given index, emitting a removal event.`Ok(tablet)` if successfully deleted, `Err(())` if out-of-bounds.
    fn delete_tcid_by_idx(&mut self, idx: i32) -> Result<&mut RawTabletSlot, ()> {
        let idx = usize::try_from(idx).map_err(|_| ())?;

        // idx is not a direct subscript into `raw_tablets`, instead, an index of `n` means
        // "the `n`th tablet which is not marked for deletion".
        // Soundness: deletion list is always deduplicated.
        if idx
            >= self
                .raw_tablets
                .len()
                .saturating_sub(self.raw_tablet_deletions.len())
        {
            Err(())
        } else {
            // `cur_logical_idx` is the actual subscript into `raw_tablets`. we need to juggle
            // these two concepts!
            let mut cur_logical_idx = 0;
            for (cur_physical_idx, raw) in self.raw_tablets.iter_mut().enumerate() {
                // Physical idx not already deleted
                if let Err(deletion_point) =
                    self.raw_tablet_deletions.binary_search(&cur_physical_idx)
                {
                    if cur_logical_idx == idx {
                        // This is our guy!
                        // `deletion_point` tells us where to store this to keep deletion list
                        // sorted. Also implicitly deduplicated since `binary_search` told us it
                        // doesn't already exist!
                        self.raw_tablet_deletions
                            .insert(deletion_point, cur_physical_idx);
                        // Emit event.
                        self.events.push(crate::events::raw::Event::Tablet {
                            tablet: ID::Tablet(raw.tcid()),
                            event: crate::events::raw::TabletEvent::Removed,
                        });
                        return Ok(raw);
                    }
                    // Not the right one, but not deleted.
                    // (deleted items don't advance the logical cursor)
                    cur_logical_idx += 1;
                }
            }
            // Fell through, checked all and it was not found.
            Err(())
        }
    }
    fn handle_packets(
        &mut self,
        rts: &tablet_pc::IRealTimeStylus,
        stylus_info: tablet_pc::StylusInfo,
        // Number of packets packed into `props``
        num_packets: u32,
        // Some number of words per packet, times number of packets.
        props: &[i32],
        phase: StylusPhase,
    ) {
        let tablet_id = ID::Tablet(stylus_info.tcid);

        // Find the relevant tool
        let Ok(tool) = Self::get_or_insert_tool(&mut self.tools, rts, stylus_info.cid) else {
            // Failed to get stylus, nothing else for us to do.
            return;
        };

        // Find the relevant tablet
        let tablet = self
            .raw_tablets
            .iter()
            .enumerate()
            .filter_map(|(physical_idx, tab)| {
                // Filter for tablets that are *not* already deleted.
                self.raw_tablet_deletions
                    .binary_search(&physical_idx)
                    .is_err()
                    .then_some(tab)
            })
            .find(|&tab| tab.tcid() == stylus_info.tcid);

        let Some(RawTabletSlot::Concrete(tablet)) = tablet else {
            // Either not found or a dummy (bad tablet) slot.
            // We cannot parse the packet, so there's nothing to do here :<
            return;
        };

        // Virally spread capabilities from tablet to any tool that visits it
        // (only if going from out to some other state to avoid redundany calcs)
        if self
            .stylus_states
            .contains_key(tool.internal_id.unwrap_ink())
        {
            tool.axes = tool.axes.union(&tablet.axes);
        }

        // Emit events.
        let stylus_id = *(tool.internal_id.unwrap_ink());
        let mut needs_frame = false;

        {
            // Check if the phase has changed, update and report the new phase if so.
            let cur_stylus_state = self.stylus_states.entry(stylus_id);

            let mut push_phase_events = |from: Option<StylusPhase>| {
                // Fairly icky.
                let e = crate::events::raw::Event::Tool {
                    tool: stylus_id,
                    // We can assume `from` and `phase` are not equal.
                    event: match phase {
                        StylusPhase::InAir => match from {
                            // Going from nowhere to in-air is an In event
                            None => crate::events::raw::ToolEvent::In { tablet: tablet_id },
                            Some(_) => crate::events::raw::ToolEvent::Up,
                        },
                        StylusPhase::Touched => match from {
                            // Going from nowhere to touched is In then Down!
                            None => {
                                self.events.push(crate::events::raw::Event::Tool {
                                    tool: stylus_id,
                                    event: crate::events::raw::ToolEvent::In { tablet: tablet_id },
                                });
                                crate::events::raw::ToolEvent::Down
                            }
                            Some(_) => crate::events::raw::ToolEvent::Down,
                        },
                    },
                };
                self.events.push(e);

                // These events we just pushed need a frame!
                needs_frame = true;
            };

            match cur_stylus_state {
                std::collections::btree_map::Entry::Vacant(v) => {
                    push_phase_events(None);
                    v.insert(phase);
                }
                std::collections::btree_map::Entry::Occupied(mut o) => {
                    if *o.get() != phase {
                        push_phase_events(Some(*o.get()));
                        o.insert(phase);
                    }
                }
            }
        }

        if let Ok(num_packets @ 1..) = usize::try_from(num_packets) {
            let props_per_packet = props.len() / num_packets;

            let mut packets = packet::Iter::new(
                &tablet.interpreter,
                self.himetric_to_logical_pixel,
                props,
                props_per_packet,
            );

            // Bail on any error.
            while let Some(Ok(packet)) = packets.next() {
                self.events.push(crate::events::raw::Event::Tool {
                    tool: stylus_id,
                    event: crate::events::raw::ToolEvent::Pose(packet.pose),
                });

                self.events.push(crate::events::raw::Event::Tool {
                    tool: stylus_id,
                    event: crate::events::raw::ToolEvent::Frame(packet.timestamp),
                });
                // We took care of the framing now.
                needs_frame = false;
            }
        }

        // Edge case - Frame is missed if we bailed before any packets could process.
        if needs_frame {
            self.events.push(crate::events::raw::Event::Tool {
                tool: stylus_id,
                event: crate::events::raw::ToolEvent::Frame(None),
            });
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum StylusPhase {
    /// In the air above the surface. This phase doesn't exist on all hardware.
    InAir,
    /// Physically against the surface
    Touched,
}

/// Helper to set an atomic flag on drop to catch an unwind or early return.
/// Use `disarm` to prevent the poison from occuring.
#[must_use = "will poison immediately if not bound"]
struct Poison<'a> {
    poisoned: &'a sync::atomic::AtomicBool,
}
impl<'a> Poison<'a> {
    /// Set the given flag to `true` on drop unless `disarm` is called.
    /// It is valid to call this recursively.
    fn new(flag: &'a sync::atomic::AtomicBool) -> Self {
        Self { poisoned: flag }
    }
    /// Prevents the poison from occuring.
    fn disarm(self) {
        // This is sound, our default drop glue is no-op so this just prevents
        // the custom Drop impl.
        std::mem::forget(self);
    }
}
impl Drop for Poison<'_> {
    fn drop(&mut self) {
        // This is the very sad path, so a bit of extra overhead is fine for not losing out
        // on generality by assuming a weaker ordering.
        self.poisoned.store(true, sync::atomic::Ordering::SeqCst);
    }
}

#[windows::core::implement(tablet_pc::IStylusAsyncPlugin, com::Marshal::IMarshal)]
struct Plugin {
    rts: tablet_pc::IRealTimeStylus,
    /// A boolean flag to tell the parent manager that an error poisoned
    /// the plugin's internal state unrecoverably and to request the manager die or reset.
    /// This is needed since the ink API requires the plugin's internal state to maintain close parity with the rts, and if some
    /// events failed to process the meanings of future events may be completely uninterpretable.
    poisoned: sync::Arc<sync::atomic::AtomicBool>,
    shared_frame: sync::Arc<sync::Mutex<DataFrame>>,
    marshaler: std::rc::Rc<std::cell::OnceCell<com::Marshal::IMarshal>>,
}
impl Plugin {
    /// Create an object to set the poison flag, requesting a reset from the outside through the [`Manager`]
    /// if not disarmed. This is useful for methods where success is required for upholding an invariant but
    /// where success is unfortunately not guaranteed. Returns an `Err(E_FAIL)` if already poisoned.
    ///
    /// See [`Plugin::poisoned`] for rationale.
    fn poison_on_drop(&self) -> WinResult<Poison<'_>> {
        self.poison_bail().map(|()| Poison::new(&self.poisoned))
    }
    /// Checks that the internal state is not poisoned. Returns `E_FAIL` if it is poisoned, which implies the internal
    /// state is potentially inconsistent.
    ///
    /// See [`Plugin::poisoned`] for rationale.
    fn poison_bail(&self) -> WinResult<()> {
        // Relaxed is ok, no memory accesses are synchronized by this atomic.
        if self.poisoned.load(sync::atomic::Ordering::Relaxed) {
            Err(E_FAIL.into())
        } else {
            Ok(())
        }
    }
}

impl tablet_pc::IStylusAsyncPlugin_Impl for Plugin {}
#[allow(non_snake_case)]
impl tablet_pc::IStylusPlugin_Impl for Plugin {
    // This is an Async plugin, meaning it runs in a low priority UI thread.
    // This is because A) we don't provide a realtime callback-based api for this so a delay is no issue,
    // and B) we are using locking logic that would be a bad idea on a high-priority system thread.

    // Since there are no Sync plugins attatched, this still recieves the data directly from the stylus
    // with no additional filtering/processing.

    fn DataInterest(&self) -> WinResult<tablet_pc::RealTimeStylusDataInterest> {
        // Called on Add initially to deternine which of the below functions
        // are to be called.

        // no impl BitOr for this bitflag newtype?!??!!?!? lol
        Ok(tablet_pc::RealTimeStylusDataInterest(
            // Devices added/removed
            tablet_pc::RTSDI_RealTimeStylusEnabled.0
                // RTSDI_StylusNew - there's no event handler for this? Would be nice but not here.
                | tablet_pc::RTSDI_TabletAdded.0
                | tablet_pc::RTSDI_TabletRemoved.0
                // In/Out
                // RTSDI_StylusInRange - Handled implicitly by packet processing!
                | tablet_pc::RTSDI_StylusOutOfRange.0
                // Axis data
                | tablet_pc::RTSDI_InAirPackets.0
                | tablet_pc::RTSDI_Packets.0
                // Buttons and nibs
                | tablet_pc::RTSDI_StylusDown.0
                | tablet_pc::RTSDI_StylusUp.0
                | tablet_pc::RTSDI_StylusButtonUp.0
                | tablet_pc::RTSDI_StylusButtonDown.0
                // DPI changes needed to properly interpret HIMETRICs as logical pixels.
                | tablet_pc::RTSDI_UpdateMapping.0,
        ))
    }

    fn StylusOutOfRange(
        &self,
        rts: Option<&tablet_pc::IRealTimeStylus>,
        _tcid: u32,
        sid: u32,
    ) -> WinResult<()> {
        self.poison_bail()?;
        // Should only ever be called with the RTS we made.
        if rts != Some(&self.rts) {
            return Err(E_INVALIDARG.into());
        }
        let mut lock = self.shared_frame.lock().unwrap();

        // Get the full ID of this (sid aint enough!)
        let Some(tool) = lock.get_tool(sid) else {
            return Ok(());
        };
        let id = *tool.internal_id.unwrap_ink();

        // Remove it from the map - missing from map represents the stylus is Out.
        let old_phase = lock.stylus_states.remove(&id);

        // The stylus was busy. Emit appropriate events to yank it away
        if let Some(old_phase) = old_phase {
            match old_phase {
                // Was touching. emit up and out.
                StylusPhase::Touched => {
                    lock.events.push(crate::events::raw::Event::Tool {
                        tool: id,
                        event: crate::events::raw::ToolEvent::Up,
                    });
                    lock.events.push(crate::events::raw::Event::Tool {
                        tool: id,
                        event: crate::events::raw::ToolEvent::Out,
                    });
                }
                // Was in air. Emit just out.
                StylusPhase::InAir => {
                    lock.events.push(crate::events::raw::Event::Tool {
                        tool: id,
                        event: crate::events::raw::ToolEvent::Out,
                    });
                }
            }
        }

        Ok(())
    }

    fn RealTimeStylusEnabled(
        &self,
        rts: Option<&tablet_pc::IRealTimeStylus>,
        num_tablets: u32,
        tcids: *const u32,
    ) -> WinResult<()> {
        // Treat this as a CTOR!

        // Poison section - we need to set our internal tablet array to match RTS or bad stuff happens.
        let poison = self.poison_on_drop()?;
        // Should only ever be called with the RTS we made.
        if rts != Some(&self.rts) {
            return Err(E_INVALIDARG.into());
        }
        let tcids = unsafe {
            match num_tablets {
                0 => &[],
                // Weird guarantee made by the docs, hm! What happens when you connect nine ink devices to
                // a windows machine???
                1..=8 => {
                    if tcids.is_null() {
                        return Err(E_POINTER.into());
                    }
                    // As cast ok - of course 8 is in range of usize :P
                    std::slice::from_raw_parts(tcids, num_tablets as usize)
                }
                _ => return Err(E_INVALIDARG.into()),
            }
        };
        let mut shared = self.shared_frame.lock().map_err(|_| E_FAIL)?;
        for &tcid in tcids {
            unsafe {
                let tablet = self.rts.GetTabletFromTabletContextId(tcid)?;
                shared.append_tablet(&self.rts, &tablet, tcid);
            }
        }

        poison.disarm();
        Ok(())
    }

    fn StylusDown(
        &self,
        rts: Option<&tablet_pc::IRealTimeStylus>,
        stylus_info: *const tablet_pc::StylusInfo,
        props_in_packet: u32,
        packet: *const i32,
        _: *mut *mut i32,
    ) -> WinResult<()> {
        self.poison_bail()?;
        if rts != Some(&self.rts) {
            return Err(E_INVALIDARG.into());
        }

        // Get the slice of data:
        let props = match props_in_packet {
            0 => &[],
            1..=32 => {
                if packet.is_null() {
                    return Err(E_POINTER.into());
                }
                unsafe {
                    // Unwrap ok, we checked it's <= 32
                    std::slice::from_raw_parts(packet, usize::try_from(props_in_packet).unwrap())
                }
            }
            // Spec says <= 32!
            _ => return Err(E_INVALIDARG.into()),
        };

        // This is not an optional field, according to spec.
        let stylus_info = unsafe { stylus_info.as_ref() }.ok_or(E_POINTER)?;
        let mut lock = self.shared_frame.lock().unwrap();
        // Delegate!
        // We don't have specific states for Up/Down transitions, those are handled
        // implicitly but observing previous state and new state.
        lock.handle_packets(&self.rts, *stylus_info, 1, props, StylusPhase::Touched);

        Ok(())
    }

    fn StylusUp(
        &self,
        rts: Option<&tablet_pc::IRealTimeStylus>,
        stylus_info: *const tablet_pc::StylusInfo,
        props_in_packet: u32,
        packet: *const i32,
        _: *mut *mut i32,
    ) -> WinResult<()> {
        self.poison_bail()?;
        if rts != Some(&self.rts) {
            return Err(E_INVALIDARG.into());
        }

        // Get the slice of data:
        let props = match props_in_packet {
            0 => &[],
            1..=32 => {
                if packet.is_null() {
                    return Err(E_POINTER.into());
                }
                unsafe {
                    // Unwrap ok, we checked it's <= 32
                    std::slice::from_raw_parts(packet, usize::try_from(props_in_packet).unwrap())
                }
            }
            // Spec says <= 32!
            _ => return Err(E_INVALIDARG.into()),
        };

        // This is not an optional field, according to spec.
        let stylus_info = unsafe { stylus_info.as_ref() }.ok_or(E_POINTER)?;
        let mut lock = self.shared_frame.lock().unwrap();
        // Delegate!
        // We don't have specific states for Up/Down transitions, those are handled
        // implicitly but observing previous state and new state.
        lock.handle_packets(&self.rts, *stylus_info, 1, props, StylusPhase::InAir);

        Ok(())
    }

    fn StylusButtonDown(
        &self,
        rts: Option<&tablet_pc::IRealTimeStylus>,
        _sid: u32,
        _button_guid: *const core::GUID,
        _stylus_position: *mut POINT,
    ) -> WinResult<()> {
        self.poison_bail()?;
        if rts != Some(&self.rts) {
            return Err(E_INVALIDARG.into());
        }

        Ok(())
    }

    fn StylusButtonUp(
        &self,
        rts: Option<&tablet_pc::IRealTimeStylus>,
        _sid: u32,
        _button_guid: *const core::GUID,
        _stylus_position: *mut POINT,
    ) -> WinResult<()> {
        self.poison_bail()?;
        if rts != Some(&self.rts) {
            return Err(E_INVALIDARG.into());
        }

        Ok(())
    }

    fn InAirPackets(
        &self,
        rts: Option<&tablet_pc::IRealTimeStylus>,
        stylus_info: *const tablet_pc::StylusInfo,
        // A note on these arguments: the meanings and names in
        // (the docs)[https://learn.microsoft.com/en-us/windows/win32/api/rtscom/nf-rtscom-istylusplugin-packets]
        // are seemingly just flatly *incorrect*, and this is clear from looking at the values in a debugger.
        // I am not the only one to have figured this out through trial and error, as
        // [C++ example code](https://gist.github.com/Kiterai/d86b3ce91eaa7256b5510ab378182684#file-main-cpp-L189)
        // using this interface online has figured out the same meanings I have (which are decidedly *not* the meanings
        // presented in the docs.)

        // As it turns out, "number of properties per packet" is actually "number of packets", and that
        // "length of the buffer in bytes" is actually "total number of LONG properties in the buffer"

        // How many packets are in the array pointed to by `props`?
        num_packets: u32,
        // How many words in length is `props`? these are divided evenly amongst the packets
        total_props: u32,
        // Pointer to the array of properties, to be evenly subdivided into `num_packets`.
        props: *const i32,
        // Output properties. We don't use this functionality.
        _: *mut u32,
        _: *mut *mut i32,
    ) -> WinResult<()> {
        self.poison_bail()?;
        if rts != Some(&self.rts) {
            return Err(E_INVALIDARG.into());
        }

        // Get the slice of data:
        let props = match total_props {
            0 => &[],
            1..=0x7FFF => {
                if props.is_null() {
                    return Err(E_POINTER.into());
                }
                unsafe {
                    // Unwrap ok, we checked it's <= 0x7FFF which is definitely fitting in usize.
                    std::slice::from_raw_parts(props, usize::try_from(total_props).unwrap())
                }
            }
            // Spec says <= 0x7FFF!
            _ => return Err(E_INVALIDARG.into()),
        };

        // This is not an optional field, according to spec.
        let stylus_info = unsafe { stylus_info.as_ref() }.ok_or(E_POINTER)?;
        let mut lock = self.shared_frame.lock().unwrap();
        // Delegate!
        lock.handle_packets(
            &self.rts,
            *stylus_info,
            num_packets,
            props,
            StylusPhase::InAir,
        );

        Ok(())
    }

    fn Packets(
        &self,
        rts: Option<&tablet_pc::IRealTimeStylus>,
        stylus_info: *const tablet_pc::StylusInfo,
        // See `InAirPackets` for reverse engineering of these argument meanings.
        num_packets: u32,
        total_props: u32,
        props: *const i32,
        _: *mut u32,
        _: *mut *mut i32,
    ) -> WinResult<()> {
        self.poison_bail()?;
        if rts != Some(&self.rts) {
            return Err(E_INVALIDARG.into());
        }

        // Get the slice of data:
        let props = match total_props {
            0 => &[],
            1..=0x7FFF => {
                if props.is_null() {
                    return Err(E_POINTER.into());
                }
                unsafe {
                    // Unwrap ok, we checked it's <= 0x7FFF which is definitely fitting in usize.
                    std::slice::from_raw_parts(props, usize::try_from(total_props).unwrap())
                }
            }
            // Spec says <= 0x7FFF!
            _ => return Err(E_INVALIDARG.into()),
        };

        // This is not an optional field, according to spec.
        let stylus_info = unsafe { stylus_info.as_ref() }.ok_or(E_POINTER)?;
        let mut lock = self.shared_frame.lock().unwrap();
        // Delegate!
        lock.handle_packets(
            &self.rts,
            *stylus_info,
            num_packets,
            props,
            StylusPhase::Touched,
        );

        Ok(())
    }

    fn TabletAdded(
        &self,
        rts: Option<&tablet_pc::IRealTimeStylus>,
        tablet: Option<&tablet_pc::IInkTablet>,
    ) -> WinResult<()> {
        // Poison section - we need to set our internal tablet array to match RTS or bad stuff happens.
        let poison = self.poison_on_drop()?;
        // Should only ever be called with the RTS we made.
        if rts != Some(&self.rts) {
            return Err(E_INVALIDARG.into());
        }

        // Can only continue if both are available
        let rts = rts.ok_or(E_POINTER)?;
        let tablet = tablet.ok_or(E_POINTER)?;

        let tcid = unsafe { rts.GetTabletContextIdFromTablet(tablet) }?;

        let mut lock = self.shared_frame.lock().map_err(|_| E_FAIL)?;
        unsafe { lock.append_tablet(&self.rts, tablet, tcid) };

        poison.disarm();
        Ok(())
    }

    fn TabletRemoved(
        &self,
        _rts: Option<&tablet_pc::IRealTimeStylus>,
        tablet_idx: i32,
    ) -> WinResult<()> {
        // Poison section - we need to set our internal tablet array to match RTS or bad stuff happens.
        let poison = self.poison_on_drop()?;

        let mut lock = self.shared_frame.lock().map_err(|_| E_FAIL)?;
        let _ = lock
            // IMPORTANT! use this method to handle all the bookkeeping. `tablet_idx`
            // is not a direct subscript into self.raw_tablets!
            .delete_tcid_by_idx(tablet_idx)
            // Out-of-bounds
            .map_err(|()| E_INVALIDARG)?;

        poison.disarm();
        Ok(())
    }

    fn UpdateMapping(&self, _: Option<&tablet_pc::IRealTimeStylus>) -> WinResult<()> {
        // Called on DPI change, need to re-fetch the conversion factor from HIMETRIC to logical pixels.
        self.poison_bail()?;
        let mut lock = self.shared_frame.lock().map_err(|_| E_FAIL)?;
        lock.himetric_to_logical_pixel = unsafe { fetch_himetric_to_logical_pixel(lock.hwnd) };

        Ok(())
    }

    // ================= Dead code :V ==================

    #[rustfmt::skip]
    fn StylusInRange(&self, _: Option<&tablet_pc::IRealTimeStylus>, _: u32, _: u32)
        -> WinResult<()> {
        // UNREACHABLE, FILTERED BY DATAINTEREST
        // Implicitly handled by packet processing. (if a stylus is Out and then suddently
        // starts producing packets again, it must've come In)
        Ok(())
    }
    #[rustfmt::skip]
    fn RealTimeStylusDisabled(&self, _: Option<&tablet_pc::IRealTimeStylus>,
        _: u32, _: *const u32) -> WinResult<()> {
        // UNREACHABLE, FILTERED BY DATAINTEREST
        Ok(())
    }
    #[rustfmt::skip]
    fn Error(&self, _: Option<&tablet_pc::IRealTimeStylus>,
        _: Option<&tablet_pc::IStylusPlugin>, _: tablet_pc::RealTimeStylusDataInterest,
        _: core::HRESULT, _: *mut isize) -> WinResult<()> {
        // UNREACHABLE, FILTERED BY DATAINTEREST
        Ok(())
    }
    #[rustfmt::skip]
    fn CustomStylusDataAdded( &self, _: Option<&tablet_pc::IRealTimeStylus>,
        _: *const core::GUID, _: u32, _: *const u8) -> WinResult<()> {
        // UNREACHABLE, FILTERED BY DATAINTEREST
        Ok(())
    }
    #[rustfmt::skip]
    fn SystemEvent(&self, _: Option<&tablet_pc::IRealTimeStylus>, _: u32, _: u32,
        _: u16, _: &tablet_pc::SYSTEM_EVENT_DATA) -> WinResult<()> {
        // UNREACHABLE, FILTERED BY DATAINTEREST
        // This includes pen gestures as recognized by the system, such as flicks.
        // The data that this plugin recieves is raw and not filtered through this recognition - this is 
        // on purpose, since so many of the Windows Ink woes are due to apps interpreting drawing as swiping.
        // (This could be implemented in the future, with a big asterisk to this crate's users to take these gestures
        // with a grain of salt due to the afforementioned false-positives.)
        Ok(())
    }
}
/// Deeply unsure about this. Loosly interpreted from [this issue](https://github.com/microsoft/windows-rs/issues/753).
/// This interface is something like "has a marshaler" and every method is supposed to delegate
/// directly to the owned marshaler. This is what i've done here, but it's reallllyy weird and the types don't
/// quite line up as well as they did in the more "raw" implementation from the issue. Please review me.
#[allow(non_snake_case)]
impl com::Marshal::IMarshal_Impl for Plugin {
    fn DisconnectObject(&self, dwreserved: u32) -> WinResult<()> {
        unsafe { self.marshaler.get().unwrap().DisconnectObject(dwreserved) }
    }
    fn GetMarshalSizeMax(
        &self,
        riid: *const core::GUID,
        pv: *const ::core::ffi::c_void,
        dwdestcontext: u32,
        pvdestcontext: *const ::core::ffi::c_void,
        mshlflags: u32,
    ) -> WinResult<u32> {
        unsafe {
            self.marshaler.get().unwrap().GetMarshalSizeMax(
                riid,
                // Why Some(..)? Am i supposed to do a null check?
                Some(pv),
                dwdestcontext,
                // Why Some(..)? Am i supposed to do a null check?
                Some(pvdestcontext),
                mshlflags,
            )
        }
    }
    fn GetUnmarshalClass(
        &self,
        riid: *const core::GUID,
        pv: *const ::core::ffi::c_void,
        dwdestcontext: u32,
        pvdestcontext: *const ::core::ffi::c_void,
        mshlflags: u32,
    ) -> WinResult<core::GUID> {
        unsafe {
            self.marshaler.get().unwrap().GetUnmarshalClass(
                riid,
                // Why Some(..)? Am i supposed to do a null check?
                Some(pv),
                dwdestcontext,
                // Why Some(..)? Am i supposed to do a null check?
                Some(pvdestcontext),
                mshlflags,
            )
        }
    }
    fn MarshalInterface(
        &self,
        pstm: ::core::option::Option<&com::IStream>,
        riid: *const core::GUID,
        pv: *const ::core::ffi::c_void,
        dwdestcontext: u32,
        pvdestcontext: *const ::core::ffi::c_void,
        mshlflags: u32,
    ) -> WinResult<()> {
        unsafe {
            self.marshaler.get().unwrap().MarshalInterface(
                pstm,
                riid,
                // Why Some(..)? Am i supposed to do a null check?
                Some(pv),
                dwdestcontext,
                // Why Some(..)? Am i supposed to do a null check?
                Some(pvdestcontext),
                mshlflags,
            )
        }
    }
    fn ReleaseMarshalData(&self, pstm: ::core::option::Option<&com::IStream>) -> WinResult<()> {
        unsafe { self.marshaler.get().unwrap().ReleaseMarshalData(pstm) }
    }
    fn UnmarshalInterface(
        &self,
        pstm: ::core::option::Option<&com::IStream>,
        riid: *const core::GUID,
        ppv: *mut *mut ::core::ffi::c_void,
    ) -> WinResult<()> {
        unsafe {
            self.marshaler
                .get()
                .unwrap()
                .UnmarshalInterface(pstm, riid, ppv)
        }
    }
}

pub struct Manager {
    /// Invariant: valid for the lifetime of Self.
    hwnd: HWND,
    /// The rts that owns the single async [`Plugin`]` instance.
    rts: tablet_pc::IRealTimeStylus,
    /// Shared with [`Plugin::poisoned`].
    poisoned: sync::Arc<sync::atomic::AtomicBool>,
    /// Shared state, written asynchronously from the plugin.
    shared_frame: sync::Arc<sync::Mutex<DataFrame>>,
    /// Cloned local copy of the shared state after a frame.
    local_frame: Option<DataFrame>,
}

impl Manager {
    /// Creates a tablet manager with from the given `HWND`.
    /// # Safety
    /// * The given `HWND` must be valid as long as the returned `Manager` is alive.
    /// * Only *one* manager may exist for this `HWND`. Claims Ink for the entire window rectangle - No other Ink collection
    ///   APIs should be enabled on this window while the returned manager exists.
    #[allow(clippy::needless_pass_by_value)]
    pub(crate) unsafe fn build_hwnd(
        opts: crate::builder::Builder,
        hwnd: std::num::NonZeroIsize,
    ) -> WinResult<Self> {
        // Safety: Uhh..
        unsafe {
            let hwnd = HWND(hwnd.get());
            // Bitwise cast from isize to usize. (`as` is not necessarily bitwise. on such platforms, is this even sound? ehhh)
            let hwnd_handle = HANDLE_PTR(std::mem::transmute::<isize, usize>(hwnd.0));

            let rts: tablet_pc::IRealTimeStylus = com::CoCreateInstance(
                std::ptr::from_ref(&tablet_pc::RealTimeStylus),
                None,
                com::CLSCTX_ALL,
            )?;

            // Many settings must have the rts disabled
            rts.SetEnabled(false)?;
            // Request all our supported axes
            rts.SetDesiredPacketDescription(packet::DESIRED_PACKET_DESCRIPTIONS)?;
            // Safety: Must survive as long as `rts`. deferred to this fn's contract.
            rts.SetHWND(hwnd_handle)?;

            let shared_frame = sync::Arc::new(sync::Mutex::new(DataFrame {
                raw_tablet_deletions: vec![],
                raw_tablets: vec![],
                tablets: vec![],
                tools: vec![],
                stylus_states: std::collections::BTreeMap::new(),
                events: vec![],
                hwnd,
                himetric_to_logical_pixel: fetch_himetric_to_logical_pixel(hwnd),
            }));

            let poisoned = sync::Arc::new(sync::atomic::AtomicBool::new(false));

            // Rc to lazily set the marshaler once we have it - struct needs to be made in order to create a marshaler,
            // but struct also needs to have the marshaler inside of it! We don't need thread safety,
            // since the refcount only changes during creation, which is single-threaded.
            let inner_marshaler = std::rc::Rc::new(std::cell::OnceCell::new());

            let plugin = tablet_pc::IStylusAsyncPlugin::from(Plugin {
                rts: rts.clone(),
                poisoned: poisoned.clone(),
                shared_frame: shared_frame.clone(),
                marshaler: inner_marshaler.clone(),
            });

            // Create a concretely typed marshaler, insert it into the plugin so that it may
            // statically disbatch to it.
            // The marshal impl and this bit of code adapted from here:
            // https://github.com/microsoft/windows-rs/issues/753
            // This was a whole thing to get working. I think i'm doing it right, but its all so
            // undocumented how can I ever be sure D:
            let marshaler: com::Marshal::IMarshal = {
                use core::ComInterface;
                use core::Interface;

                let marshaler = com::CoCreateFreeThreadedMarshaler(&plugin)?;
                let unknown: core::IUnknown = std::mem::transmute_copy(&marshaler);

                let mut marshaler = std::ptr::null_mut();
                (unknown.vtable().QueryInterface)(
                    std::mem::transmute_copy(&unknown),
                    &com::Marshal::IMarshal::IID,
                    &mut marshaler,
                )
                // *Should* be infallible, but this is a safety condition so just make really extra sure.
                .unwrap();
                std::mem::transmute_copy(&marshaler)
            };

            // Infallible, this is the only instance of set being called.
            inner_marshaler.set(marshaler).unwrap();

            // Ensure we are the only plugin - we want the rawest data possible!
            rts.RemoveAllStylusSyncPlugins()?;
            rts.RemoveAllStylusAsyncPlugins()?;
            // Insert at the top of the async plugin list.
            // See top of `IStylusPlugin_Impl for Plugin` for notes on async vs sync.
            rts.AddStylusAsyncPlugin(0, &plugin)?;

            // Apply builder settings
            {
                let crate::builder::Builder {
                    emulate_tool_from_mouse,
                } = opts;

                rts.SetAllTabletsMode(emulate_tool_from_mouse)?;
            }

            // We're ready, startup async event collection!
            rts.SetEnabled(true)?;

            Ok(Self {
                hwnd,
                rts,
                poisoned,
                shared_frame,
                local_frame: None,
            })
        }
    }
    /// Attempt to recover a poisoned plugin.
    pub fn handle_poison(&mut self) -> Result<(), ()> {
        unsafe {
            self.rts.SetEnabled(false).map_err(|_| ())?;
            self.rts.ClearStylusQueues().map_err(|_| ())?;
        }

        let scale_factor = unsafe { fetch_himetric_to_logical_pixel(self.hwnd) };
        // Make shared state consistent.
        self.shared_frame
            .lock()
            // Lock could also be poisoned! We don't mind, nothing in there can be trusted anyway.
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            // Clear all state, which might be complete bogus.
            .reset()
            .himetric_to_logical_pixel = scale_factor;

        // Reset cached internal state too.
        if let Some(frame) = &mut self.local_frame {
            frame.reset();
            frame.himetric_to_logical_pixel = scale_factor;
        }

        // Clear the poison state.
        // We need the disable and clear to go through before the enable.
        self.poisoned.store(false, sync::atomic::Ordering::SeqCst);

        unsafe {
            self.rts.SetEnabled(true).map_err(|_| ())?;
        }

        Ok(())
    }
}

impl Drop for Manager {
    fn drop(&mut self) {
        unsafe {
            // Disable and destroy the RTS and all plugins.
            // This is needed since the plugin holds a strong ref to the RTS which holds a
            // strong ref to the plugin - a reference loop! This breaks that reference loop
            // and allows both the drop glue to free both.

            // Also, disabling prevents UB when a manager is dropped and another is re-made
            // on the same window in case for somereason the drop doesn't work :3
            let _ = self.rts.SetEnabled(false);
            let _ = self.rts.RemoveAllStylusAsyncPlugins();
        }
    }
}

impl super::PlatformImpl for Manager {
    fn pump(&mut self) -> Result<(), crate::PumpError> {
        if self.poisoned.load(sync::atomic::Ordering::Relaxed) {
            self.handle_poison().map_err(|()| todo!())
        } else {
            // Lock and clone the inner state for this frame.
            // We clone since the user can borrow this data for unbounded amount of time before next frame,
            // and we don't want to lock out the callbacks from writing new data.
            if let Ok(mut lock) = self.shared_frame.lock() {
                if let Some(local_frame) = self.local_frame.as_mut() {
                    // Last frame exists, clone_from to reuse allocs
                    local_frame.clone_from(&lock);
                } else {
                    // Last frame doesn't exist, clone anew!
                    self.local_frame = Some(lock.clone());
                }

                lock.frame_end_cleanup();
            } else {
                // Failed to lock!
                self.local_frame = None;
            }

            Ok(())
        }
    }
    fn timestamp_granularity(&self) -> Option<std::time::Duration> {
        // Tablets optionally report, which *seems* to be in milliseconds. There is no unit enumeration for Time,
        // and the `GUID_PACKETPROPERTY_GUID_TIMER_TICK` is only described as `The time the packet was generated`
        Some(std::time::Duration::from_millis(1))
    }

    // ================ Dispatches to inner frame!
    fn pads(&self) -> &[crate::pad::Pad] {
        // Ink doesn't report any of these capabilities or events :<
        // The situation for ring, pad buttons, and sliders is dire on windows, with drivers e.g. just emulating
        // keypresses from swipes on the ring. Oof!
        &[]
    }
    fn tools(&self) -> &[crate::tool::Tool] {
        self.local_frame.as_ref().map_or(&[], DataFrame::tools)
    }
    fn tablets(&self) -> &[crate::tablet::Tablet] {
        self.local_frame.as_ref().map_or(&[], DataFrame::tablets)
    }
    fn make_summary(&self) -> crate::events::summary::Summary {
        crate::events::summary::Summary::empty()
    }
    fn raw_events(&self) -> super::RawEventsIter<'_> {
        super::RawEventsIter::Ink(
            self.local_frame
                .as_ref()
                // Events if available, fallback on default (empty) slice.
                .map_or(Default::default(), DataFrame::raw_events)
                .iter(),
        )
    }
}
