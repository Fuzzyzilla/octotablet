use windows::core::{self, Result as WinResult};
use windows::Win32::Foundation::{E_FAIL, E_INVALIDARG, E_POINTER, HANDLE_PTR, POINT};
use windows::Win32::System::Com as com;
use windows::Win32::UI::TabletPC as tablet_pc;

#[derive(Clone, Copy, Hash, PartialEq, Eq)]
pub struct ID {
    /// The actual `tcid` or `cid` ect from windows
    id: u32,
    /// Some user data to differentiate different conceptual objects with the same windows ID.
    /// For example, tip and eraser are the same ID, use this to differentiate.
    data: u32,
}

/// All the axes we care about ([`crate::tool::Axis`])
/// (There are Azimuth and altitude axes that can be used to derive X/Y Tilt.
/// Are there any devices that report azimuth/altitude and NOT x/y? shoud i implement that?)
const DESIRED_PACKET_DESCRIPTIONS: &[core::GUID] = &[
    // X, Y always reported regardless of if they're requested, and always in first and second positions
    // tablet_pc::GUID_PACKETPROPERTY_GUID_X,
    // tablet_pc::GUID_PACKETPROPERTY_GUID_Y,
    // Other axes follow their indices into this list to determine where they lay in the resulting packets
    tablet_pc::GUID_PACKETPROPERTY_GUID_Z,
    tablet_pc::GUID_PACKETPROPERTY_GUID_DEVICE_CONTACT_ID,
    tablet_pc::GUID_PACKETPROPERTY_GUID_NORMAL_PRESSURE,
    tablet_pc::GUID_PACKETPROPERTY_GUID_X_TILT_ORIENTATION,
    tablet_pc::GUID_PACKETPROPERTY_GUID_Y_TILT_ORIENTATION,
    tablet_pc::GUID_PACKETPROPERTY_GUID_TWIST_ORIENTATION,
    tablet_pc::GUID_PACKETPROPERTY_GUID_TIMER_TICK,
    // Packet status always reported last regardless of it's index into this list, but still must be requested.
    tablet_pc::GUID_PACKETPROPERTY_GUID_PACKET_STATUS,
];

/// The full inner state
struct DataFrame {
    tools: Vec<crate::tool::Tool>,
    tablets: Vec<crate::tablet::Tablet>,
    events: Vec<crate::events::raw::Event<ID>>,
}
impl Clone for DataFrame {
    fn clone(&self) -> Self {
        // I hope the compiler makes this implementation less stupid :D
        let mut clone = Self::empty();
        clone.clone_from(self);
        clone
    }
    fn clone_from(&mut self, source: &Self) {
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
    }
}
impl DataFrame {
    fn empty() -> Self {
        Self {
            tools: Vec::new(),
            tablets: Vec::new(),
            events: Vec::new(),
        }
    }
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
    fn make_summary(&self) -> crate::events::summary::Summary {
        crate::events::summary::Summary::empty()
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
                available_axes: crate::tool::AvailableAxes::empty(),
                position_info: crate::tool::AxisInfo { precision: None },
                axis_info: Default::default(),
                distance_unit: crate::tool::DistanceUnit::Unitless,
            });
            self.tools.last_mut().unwrap()
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum StylusPhase {
    /// In the air above the surface. This phase doesn't exist on all hardware.
    InAir,
    /// Physically against the surface
    Touched,
    /// Going from any to `Touched``
    TouchEnter,
    /// Going from `Touched` to any`
    TouchLeave,
}

#[derive(Copy, Clone, Debug)]
struct Packet<'a> {
    position: [i32; 2],
    props: &'a [i32],
    status: i32,
}
struct PacketIter<'a> {
    props: &'a [i32],
    props_per_packet: usize,
}
impl<'a> PacketIter<'a> {
    /// Create an iterator over the packets of a slice of data. `None` if the `props_per_packet` is insufficient
    /// for a full packet to be constructed (< 3).
    fn new(props: &'a [i32], props_per_packet: usize) -> Option<Self> {
        if props_per_packet < 3 {
            return None;
        }
        // Round down to nearest whole packet
        let trim = (props.len() / props_per_packet) * props_per_packet;
        let props = &props[..trim];
        Some(Self {
            props,
            props_per_packet,
        })
    }
}
impl<'a> Iterator for PacketIter<'a> {
    type Item = Packet<'a>;
    fn next(&mut self) -> Option<Self::Item> {
        // Ctor postcondition:
        assert!(self.props_per_packet >= 3);
        // Get the next words...
        let packet = self.props.get(..self.props_per_packet)?;
        // Trim the words from input array... (wont panic, as above would have
        // short-circuited)
        self.props = &self.props[self.props_per_packet..];
        Some(Packet {
            // X, Y guaranteed by RTS to be first
            position: [packet[0], packet[1]],
            // May be empty, but never panics.
            props: &packet[2..packet.len() - 1],
            // Status guaranteed by RTS to be last
            status: *packet.last().unwrap(),
        })
    }
}

#[windows::core::implement(tablet_pc::IStylusSyncPlugin, com::Marshal::IMarshal)]
struct Plugin {
    shared_frame: std::sync::Arc<std::sync::Mutex<DataFrame>>,
    marshaler: std::rc::Rc<std::cell::OnceCell<com::Marshal::IMarshal>>,
}
impl Plugin {
    fn handle_packets(
        &self,
        rts: Option<&tablet_pc::IRealTimeStylus>,
        stylus_info: tablet_pc::StylusInfo,
        // Number of packets packed into `props``
        num_packets: u32,
        // Some number of words per packet, times number of packets.
        props: &[i32],
        phase: StylusPhase,
    ) {
        // Iterate over the packets, or None if no packets or invalid.
        let packets = usize::try_from(num_packets)
            .ok()
            .and_then(|num_packets| props.len().checked_div(num_packets))
            .and_then(|props_per_packet| PacketIter::new(props, props_per_packet));

        if let Some(packets) = packets {
            println!("{phase:?} {:#?}", &packets.collect::<Vec<_>>());
        }
    }
}
impl tablet_pc::IStylusSyncPlugin_Impl for Plugin {}
impl tablet_pc::IStylusPlugin_Impl for Plugin {
    fn DataInterest(&self) -> WinResult<tablet_pc::RealTimeStylusDataInterest> {
        // Called on Add initially to deternine which of the below functions
        // are to be called.

        // no impl BitOr for this bitflag newtype?!??!!?!? lol
        Ok(tablet_pc::RealTimeStylusDataInterest(
            // Devices added/removed
            tablet_pc::RTSDI_StylusNew.0
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

        let id = unsafe { rts.GetTabletContextIdFromTablet(tablet)? };
        let id = ID { id, data: 0 };

        let tablet = crate::tablet::Tablet {
            internal_id: id.into(),
            name: Some(unsafe { tablet.Name() }?.to_string()),
            usb_id: None,
        };

        let mut lock = self.shared_frame.lock().map_err(|_| E_FAIL)?;
        lock.tablets.push(tablet);
        lock.events.push(crate::events::raw::Event::Tablet {
            tablet: id,
            event: crate::events::raw::TabletEvent::Added,
        });

        Ok(())
    }

    fn TabletRemoved(
        &self,
        rts: Option<&tablet_pc::IRealTimeStylus>,
        tablet_idx: i32,
    ) -> WinResult<()> {
        let rts = rts.ok_or(E_POINTER)?;
        let tablet_idx = usize::try_from(tablet_idx).map_err(|_| E_INVALIDARG)?;

        // This assumes the ID is removed from the internal collection *after* this TabletRemoved
        // callback. That would make sense, but is it correct?
        // How is this not a massive race condition? lmao
        let tablets = unsafe {
            // Query the size and pointer of the internal array of IDs.
            let mut count = 0u32;
            let mut id_array = std::ptr::null_mut();
            rts.GetAllTabletContextIds(
                std::ptr::addr_of_mut!(count),
                std::ptr::addr_of_mut!(id_array),
            )?;

            // There's probably a more correct HRESULT for this
            // but after a few minutes of searching i gave up.
            let count = usize::try_from(count).map_err(|_| E_FAIL)?;

            // Turn it into a slice!
            if count == 0 || id_array.is_null() {
                &[]
            } else {
                std::slice::from_raw_parts(id_array, count)
            }
        };

        let Some(&removed_id) = tablets.get(tablet_idx) else {
            return Ok(());
        };
        let removed_id = ID {
            id: removed_id,
            data: 0,
        };

        let mut lock = self.shared_frame.lock().map_err(|_| E_FAIL)?;
        lock.tablets
            .retain(|tab| tab.internal_id != removed_id.into());
        lock.events.push(crate::events::raw::Event::Tablet {
            tablet: removed_id,
            event: crate::events::raw::TabletEvent::Removed,
        });

        Ok(())
    }

    // ================= Dead code :V ==================
    #[rustfmt::skip]
    fn RealTimeStylusEnabled(&self, _: Option<&tablet_pc::IRealTimeStylus>,
        _: u32, _: *const u32) -> WinResult<()> {
        // UNREACHABLE, FILTERED BY DATAINTEREST
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
    _rts: tablet_pc::IRealTimeStylus,
    // Shared state, written asynchronously from the plugin.
    shared_frame: std::sync::Arc<std::sync::Mutex<DataFrame>>,
    // Cloned local copy of the shared state after a frame.
    local_frame: Option<DataFrame>,
    _plugin: tablet_pc::IStylusSyncPlugin,
}
impl Manager {
    /// Creates a tablet manager with from the given `HWND`.
    /// # Safety
    /// The given `HWND` must be valid as long as the returned `Manager` is alive.
    pub(crate) unsafe fn build_hwnd(hwnd: std::num::NonZeroIsize) -> WinResult<Self> {
        // Safety: Uhh..
        unsafe {
            // Bitwise cast from isize to usize.
            let hwnd = HANDLE_PTR(std::mem::transmute::<isize, usize>(hwnd.get()));

            let rts: tablet_pc::IRealTimeStylus = com::CoCreateInstance(
                std::ptr::from_ref(&tablet_pc::RealTimeStylus),
                None,
                com::CLSCTX_ALL,
            )?;
            // Many settings must have the rts disabled
            rts.SetEnabled(false)?;
            // Request all our supported axes
            rts.SetDesiredPacketDescription(DESIRED_PACKET_DESCRIPTIONS)?;
            // Safety: Must survive as long as `rts`. deferred to this fn's contract.
            rts.SetHWND(hwnd)?;

            // Rc to lazily set the marshaler once we have it - struct needs to be made in order to create a marshaler,
            // but struct also needs to have the marshaler inside of it! We don't need thread safety,
            // since the refcount only changes during creation, which is single-threaded.
            let shared_frame = std::sync::Arc::new(std::sync::Mutex::new(DataFrame::empty()));
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
            // We're ready, startup async event collection!
            rts.SetEnabled(true)?;

            Ok(Self {
                _rts: rts,
                shared_frame,
                local_frame: None,
                _plugin: plugin,
            })
        }
    }
}
impl super::PlatformImpl for Manager {
    fn pump(&mut self) -> Result<(), crate::PumpError> {
        // Lock and clone the inner state for this frame.
        // We clone since the user can borrow this data for unbounded amount of time before next frame,
        // and we don't want to lock out the callbacks from writing new data.
        if let Ok(lock) = self.shared_frame.lock() {
            if let Some(local_frame) = self.local_frame.as_mut() {
                // Last frame exists, clone_from to reuse allocs
                local_frame.clone_from(&lock);
            } else {
                // Last frame doesn't exist, clone anew!
                self.local_frame = Some(lock.clone());
            }
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
        self.local_frame.as_ref().map_or_else(
            crate::events::summary::Summary::empty,
            DataFrame::make_summary,
        )
    }
    fn raw_events(&self) -> super::RawEventsIter<'_> {
        super::RawEventsIter::Ink(
            self.local_frame
                .as_ref()
                .map_or(&[][..], DataFrame::raw_events)
                .iter(),
        )
    }
}
