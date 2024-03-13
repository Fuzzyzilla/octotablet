//! Implementation details for Window's Ink `RealTimeStylus` interface.
//!
//! Within this module, it is sound to assume `cfg(ink_rts) == true`
//! (compiling for a wayland target + has deps, or is building docs).

use windows::core::{self, Result as WinResult};
use windows::Win32::Foundation::{E_FAIL, E_INVALIDARG, E_POINTER, HANDLE_PTR, HWND, POINT};
use windows::Win32::System::Com as com;
use windows::Win32::UI::TabletPC as tablet_pc;

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

#[derive(Clone, Copy, Hash, PartialEq, Eq)]
pub struct ID {
    /// The actual `tcid` or `cid` ect from windows
    id: u32,
    /// Some user data to differentiate different conceptual objects with the same windows ID.
    /// For example, tip and eraser are the same ID, use this to differentiate.
    data: u32,
}

/// The full inner state
struct DataFrame {
    /// cached value from [`fetch_himetric_to_logical_pixel`], updated frequently.
    himetric_to_logical_pixel: f32,
    tools: Vec<crate::tool::Tool>,
    /// Tablets *in initialization order*.
    /// This is important, since notifications may refer to a tablet by it's index.
    tablets: Vec<crate::tablet::Tablet>,
    events: Vec<crate::events::raw::Event<ID>>,
    phase: Option<StylusPhase>,
}
impl Clone for DataFrame {
    fn clone(&self) -> Self {
        // I hope the compiler makes this implementation less stupid :D
        let mut clone = Self {
            himetric_to_logical_pixel: 0.0,
            // Empty vecs don't alloc, this is ok.
            tools: vec![],
            tablets: vec![],
            events: vec![],
            phase: None,
        };

        clone.clone_from(self);
        clone
    }
    fn clone_from(&mut self, source: &Self) {
        self.himetric_to_logical_pixel = source.himetric_to_logical_pixel;
        self.tools.clear();
        self.tools.extend(
            source
                .tools
                .iter()
                // Intentionally !Clone, manually impl:
                .map(|tool| crate::tool::Tool {
                    // Clone what needs to be:
                    internal_id: tool.internal_id.clone(),
                    // Copy the rest:
                    ..*tool
                }),
        );

        self.tablets.clear();
        self.tablets.extend(
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

        self.events.clone_from(&source.events);
        self.phase.clone_from(&source.phase);
    }
}
impl DataFrame {
    fn pads(_: &Self) -> &[crate::pad::Pad] {
        // Ink doesn't report any of these capabilities or events :<
        &[]
    }
    fn tools(&self) -> &[crate::tool::Tool] {
        &self.tools
    }
    fn tablets(&self) -> &[crate::tablet::Tablet] {
        &self.tablets
    }
    fn raw_events(&self) -> &[crate::events::raw::Event<ID>] {
        &self.events
    }
    fn tool_or_insert(&mut self, stylus_info: tablet_pc::StylusInfo) -> &mut crate::tool::Tool {
        let internal_id: super::InternalID = ID {
            id: stylus_info.cid,
            data: u32::from(stylus_info.bIsInvertedCursor.as_bool()),
        }
        .into();
        if let Some(pos) = self
            .tools
            .iter()
            .position(|tool| tool.internal_id == internal_id)
        {
            &mut self.tools[pos]
        } else {
            self.tools.push(crate::tool::Tool {
                internal_id,
                hardware_id: None,
                wacom_id: None,
                tool_type: None,
                axes: crate::axis::FullInfo::default(),
            });
            self.tools.last_mut().unwrap()
        }
    }
    #[allow(clippy::needless_pass_by_value)]
    fn append_tablet(&mut self, tablet: tablet_pc::IInkTablet, tcid: u32) -> WinResult<()> {
        let id = ID { id: tcid, data: 0 };

        let tablet = crate::tablet::Tablet {
            internal_id: id.into(),
            name: Some(unsafe { tablet.Name() }?.to_string()),
            usb_id: None,
        };

        self.tablets.push(tablet);
        self.events.push(crate::events::raw::Event::Tablet {
            tablet: id,
            event: crate::events::raw::TabletEvent::Added,
        });
        Ok(())
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum StylusPhase {
    /// In the air above the surface. This phase doesn't exist on all hardware.
    InAir,
    /// Physically against the surface
    Touched,
    /// Going from any to `Touched`
    TouchEnter,
    /// Going from `Touched` to any
    TouchLeave,
}

#[windows::core::implement(tablet_pc::IStylusSyncPlugin, com::Marshal::IMarshal)]
struct Plugin {
    shared_frame: std::sync::Arc<std::sync::Mutex<DataFrame>>,
    marshaler: std::rc::Rc<std::cell::OnceCell<com::Marshal::IMarshal>>,
}
impl Plugin {
    fn handle_packets(
        &self,
        _rts: Option<&tablet_pc::IRealTimeStylus>,
        _stylus_info: tablet_pc::StylusInfo,
        // Number of packets packed into `props``
        _num_packets: u32,
        // Some number of words per packet, times number of packets.
        props: &[i32],
        phase: StylusPhase,
    ) {
        use crate::util::NicheF32;
        const NULL_ID: ID = ID { data: 0, id: 0 };

        // Just take first packet which is always lead with x,y
        let &[x, y, pressure, ..] = props else {
            return;
        };

        // Stinky! Lock in hi-pri thread!!
        let mut shared = self.shared_frame.lock().unwrap();
        let himetric_to_logical_pixel = shared.himetric_to_logical_pixel;

        // If changed, report new state. Note we never go out lmao.
        if Some(phase) != shared.phase {
            // First event, mark in
            if shared.phase.is_none() {
                shared.events.push(crate::events::raw::Event::Tool {
                    tool: NULL_ID,
                    event: crate::events::raw::ToolEvent::In { tablet: NULL_ID },
                });
            }

            match phase {
                StylusPhase::TouchEnter | StylusPhase::Touched => {
                    shared.events.push(crate::events::raw::Event::Tool {
                        tool: NULL_ID,
                        event: crate::events::raw::ToolEvent::Down,
                    });
                }
                StylusPhase::TouchLeave | StylusPhase::InAir => {
                    shared.events.push(crate::events::raw::Event::Tool {
                        tool: NULL_ID,
                        event: crate::events::raw::ToolEvent::Up,
                    });
                }
            }

            shared.phase = Some(phase);
        }

        #[allow(clippy::cast_precision_loss)]
        shared.events.push(crate::events::raw::Event::Tool {
            tool: NULL_ID,
            event: crate::events::raw::ToolEvent::Pose(crate::axis::Pose {
                position: [
                    x as f32 * himetric_to_logical_pixel,
                    y as f32 * himetric_to_logical_pixel,
                ],
                distance: NicheF32::NONE,
                pressure: NicheF32::new_some(pressure as f32 / 4095.0).unwrap(),
                tilt: None,
                roll: NicheF32::NONE,
                wheel: None,
                slider: NicheF32::NONE,
                button_pressure: NicheF32::NONE,
                contact_size: None,
            }),
        });

        shared.events.push(crate::events::raw::Event::Tool {
            tool: NULL_ID,
            event: crate::events::raw::ToolEvent::Frame(None),
        });
    }
}

impl tablet_pc::IStylusSyncPlugin_Impl for Plugin {}
#[allow(non_snake_case)]
impl tablet_pc::IStylusPlugin_Impl for Plugin {
    // Important: This is a *sync* plugin, meaning it is called directly in the system's own
    // high-priority event processing loop. Keep it quick, consequences are dire!
    // Async plugins are called in application space so that limit is lifted, but I haven't
    // been able to figure out synchronization with the RTS object yet.

    fn DataInterest(&self) -> WinResult<tablet_pc::RealTimeStylusDataInterest> {
        // Called on Add initially to deternine which of the below functions
        // are to be called.

        // no impl BitOr for this bitflag newtype?!??!!?!? lol
        Ok(tablet_pc::RealTimeStylusDataInterest(
            // Devices added/removed
            tablet_pc::RTSDI_RealTimeStylusEnabled.0
                | tablet_pc::RTSDI_StylusNew.0
                | tablet_pc::RTSDI_TabletAdded.0
                | tablet_pc::RTSDI_TabletRemoved.0
                // In/Out
                | tablet_pc::RTSDI_StylusInRange.0
                | tablet_pc::RTSDI_StylusOutOfRange.0
                // Axis data
                | tablet_pc::RTSDI_InAirPackets.0
                | tablet_pc::RTSDI_Packets.0
                // Buttons and nibs
                | tablet_pc::RTSDI_StylusDown.0
                | tablet_pc::RTSDI_StylusUp.0
                | tablet_pc::RTSDI_StylusButtonUp.0
                | tablet_pc::RTSDI_StylusButtonDown.0,
        ))
    }

    fn StylusInRange(
        &self,
        _rts: Option<&tablet_pc::IRealTimeStylus>,
        _tablet_id: u32,
        _stylus_id: u32,
    ) -> WinResult<()> {
        Ok(())
    }

    fn StylusOutOfRange(
        &self,
        _rts: Option<&tablet_pc::IRealTimeStylus>,
        _tablet_id: u32,
        _stylus_id: u32,
    ) -> WinResult<()> {
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
        // Delegate!
        self.handle_packets(rts, *stylus_info, 1, props, StylusPhase::TouchEnter);

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
        // Delegate!
        self.handle_packets(rts, *stylus_info, 1, props, StylusPhase::TouchLeave);

        Ok(())
    }

    fn StylusButtonDown(
        &self,
        _rts: Option<&tablet_pc::IRealTimeStylus>,
        _stylus_id: u32,
        _button_guid: *const core::GUID,
        _stylus_position: *mut POINT,
    ) -> WinResult<()> {
        Ok(())
    }

    fn StylusButtonUp(
        &self,
        _rts: Option<&tablet_pc::IRealTimeStylus>,
        _stylus_id: u32,
        _button_guid: *const core::GUID,
        _stylus_position: *mut POINT,
    ) -> WinResult<()> {
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
        _: *mut u32,
        _: *mut *mut i32,
    ) -> WinResult<()> {
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
        // Delegate!
        self.handle_packets(rts, *stylus_info, num_packets, props, StylusPhase::InAir);

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
        // Delegate!
        self.handle_packets(rts, *stylus_info, num_packets, props, StylusPhase::Touched);

        Ok(())
    }

    fn TabletAdded(
        &self,
        rts: Option<&tablet_pc::IRealTimeStylus>,
        tablet: Option<&tablet_pc::IInkTablet>,
    ) -> WinResult<()> {
        // Can only continue if both are available
        let rts = rts.ok_or(E_POINTER)?;
        let tablet = tablet.ok_or(E_POINTER)?;

        let tcid = unsafe { rts.GetTabletContextIdFromTablet(tablet) }?;

        let mut lock = self.shared_frame.lock().map_err(|_| E_FAIL)?;
        lock.append_tablet(tablet.clone(), tcid)
    }

    fn TabletRemoved(
        &self,
        _rts: Option<&tablet_pc::IRealTimeStylus>,
        tablet_idx: i32,
    ) -> WinResult<()> {
        let tablet_idx = usize::try_from(tablet_idx).map_err(|_| E_INVALIDARG)?;

        let mut lock = self.shared_frame.lock().map_err(|_| E_FAIL)?;
        if tablet_idx >= lock.tablets.len() {
            return Err(E_INVALIDARG.into());
        }
        // Todo, this shoud be deferred to the next frame, not eagerly destroyed.
        let removed = lock.tablets.remove(tablet_idx);

        lock.events.push(crate::events::raw::Event::Tablet {
            tablet: *removed.internal_id.unwrap_ink(),
            event: crate::events::raw::TabletEvent::Removed,
        });

        Ok(())
    }
    fn RealTimeStylusEnabled(
        &self,
        rts: Option<&tablet_pc::IRealTimeStylus>,
        num_tablets: u32,
        tcids: *const u32,
    ) -> WinResult<()> {
        let Some(rts) = rts else {
            return Err(E_POINTER.into());
        };
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
            let tablet = unsafe { rts.GetTabletFromTabletContextId(tcid) }?;
            shared.append_tablet(tablet, tcid)?;
        }
        Ok(())
    }

    // ================= Dead code :V ==================
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
    fn UpdateMapping(&self, _: Option<&tablet_pc::IRealTimeStylus>) -> WinResult<()> {
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
        // It seems to echo the X, Y, buttonstate, is-inverted data which is reported by the packets anyway.
        // Additionally Includes keycodes and modifiers which is not something this crate listens for.
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
    rts: tablet_pc::IRealTimeStylus,
    // Shared state, written asynchronously from the plugin.
    shared_frame: std::sync::Arc<std::sync::Mutex<DataFrame>>,
    // Cloned local copy of the shared state after a frame.
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

            let shared_frame = std::sync::Arc::new(std::sync::Mutex::new(DataFrame {
                tablets: vec![crate::tablet::Tablet {
                    internal_id: crate::platform::InternalID::Ink(ID { id: 0, data: 0 }),
                    name: None,
                    usb_id: None,
                }],
                tools: vec![crate::tool::Tool {
                    internal_id: crate::platform::InternalID::Ink(ID { id: 0, data: 0 }),
                    hardware_id: None,
                    wacom_id: None,
                    tool_type: None,
                    axes: crate::axis::FullInfo {
                        pressure: Some(crate::axis::NormalizedInfo { granularity: None }),
                        ..Default::default()
                    },
                }],
                events: vec![],
                phase: None,
                himetric_to_logical_pixel: fetch_himetric_to_logical_pixel(hwnd),
            }));

            // Rc to lazily set the marshaler once we have it - struct needs to be made in order to create a marshaler,
            // but struct also needs to have the marshaler inside of it! We don't need thread safety,
            // since the refcount only changes during creation, which is single-threaded.
            let inner_marshaler = std::rc::Rc::new(std::cell::OnceCell::new());

            let plugin = tablet_pc::IStylusSyncPlugin::from(Plugin {
                marshaler: inner_marshaler.clone(),
                shared_frame: shared_frame.clone(),
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

            // Insert at the top of the plugin list.
            rts.AddStylusSyncPlugin(0, &plugin)?;

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
                shared_frame,
                local_frame: None,
            })
        }
    }
}

impl Drop for Manager {
    fn drop(&mut self) {
        unsafe {
            // Disable and destroy the RTS and all plugins.
            // This should hopefully cause the RTS and my plugin to be freed?
            // It does, at the very least, prevent UB when a manager is dropped and another is re-made :3
            let _ = self.rts.SetEnabled(false);
            let _ = self.rts.RemoveAllStylusSyncPlugins();
        }
    }
}

impl super::PlatformImpl for Manager {
    fn pump(&mut self) -> Result<(), crate::PumpError> {
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

            // While we have access, update the DPI. This is intended to not be cached and to be
            // cheap to execute as per microsoft's docs, but I don't want to take any risks on the
            // hi-priority system thread the plugin lives in!!
            // Safety: invariant - hwnd always valid for lifetime of Self.
            lock.himetric_to_logical_pixel = unsafe { fetch_himetric_to_logical_pixel(self.hwnd) };
            // And consume all the events!
            lock.events.clear();
        } else {
            // Failed to lock!
            self.local_frame = None;
        }

        Ok(())
    }
    fn timestamp_granularity(&self) -> Option<std::time::Duration> {
        // Ink RTS reports, which *seems* to be in milliseconds. There is no unit enumeration for Time,
        // and the `GUID_PACKETPROPERTY_GUID_TIMER_TICK` is only described as `The time the packet was generated`
        Some(std::time::Duration::from_millis(1))
    }

    // ================ Dispatches to inner frame!
    fn pads(&self) -> &[crate::pad::Pad] {
        self.local_frame.as_ref().map_or(&[], DataFrame::pads)
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
