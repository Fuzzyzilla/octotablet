//! The `IStylusPlugin` + `IMarshal` COM interface implementations on Plugin for it to act as a stylus plugin.

use windows::core::{self, Result as WinResult};
use windows::Win32::Foundation::{E_FAIL, E_INVALIDARG, E_POINTER, POINT};
use windows::Win32::System::Com as com;
use windows::Win32::UI::TabletPC as tablet_pc;

use super::{fetch_himetric_to_logical_pixel, ButtonID, Plugin, StylusPhase};

use std::{panic::AssertUnwindSafe, sync::PoisonError};

impl Plugin {
    /// It is UB to allow a rust unwind to cross FFI boundary.
    /// Catch panics, transforming them into an [`E_FAIL`] and setting the poison bit.
    fn panic_wrapper(
        &self,
        f: impl FnOnce() -> WinResult<()> + std::panic::UnwindSafe,
    ) -> WinResult<()> {
        std::panic::catch_unwind(f).unwrap_or_else(|_| {
            // Panic occured, oison unconditionally
            // Is this a good behavior? Revisit on err handling refactor >w>;;
            let _ = self.poison_on_drop();
            Err(E_FAIL.into())
        })
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
        self.panic_wrapper(AssertUnwindSafe(|| {
            self.poison_bail()?;
            // Should only ever be called with the RTS we made.
            if rts != Some(&self.rts) {
                return Err(E_INVALIDARG.into());
            }
            let mut lock = self
                .shared_frame
                .lock()
                .unwrap_or_else(PoisonError::into_inner);

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
        }))
    }

    fn RealTimeStylusEnabled(
        &self,
        rts: Option<&tablet_pc::IRealTimeStylus>,
        num_tablets: u32,
        tcids: *const u32,
    ) -> WinResult<()> {
        self.panic_wrapper(AssertUnwindSafe(|| {
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
            let mut lock = self
                .shared_frame
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            for &tcid in tcids {
                unsafe {
                    let tablet = self.rts.GetTabletFromTabletContextId(tcid)?;
                    lock.append_tablet(&self.rts, &tablet, tcid);
                }
            }

            poison.disarm();
            Ok(())
        }))
    }

    fn StylusDown(
        &self,
        rts: Option<&tablet_pc::IRealTimeStylus>,
        stylus_info: *const tablet_pc::StylusInfo,
        props_in_packet: u32,
        packet: *const i32,
        _: *mut *mut i32,
    ) -> WinResult<()> {
        self.panic_wrapper(AssertUnwindSafe(|| {
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
                        std::slice::from_raw_parts(
                            packet,
                            usize::try_from(props_in_packet).unwrap(),
                        )
                    }
                }
                // Spec says <= 32!
                _ => return Err(E_INVALIDARG.into()),
            };

            // This is not an optional field, according to spec.
            let stylus_info = unsafe { stylus_info.as_ref() }.ok_or(E_POINTER)?;
            let mut lock = self
                .shared_frame
                .lock()
                .unwrap_or_else(PoisonError::into_inner);

            // Delegate!
            // We don't have specific states for Up/Down transitions, those are handled
            // implicitly but observing previous state and new state.
            lock.handle_packets(&self.rts, *stylus_info, 1, props, StylusPhase::Touched);

            Ok(())
        }))
    }

    fn StylusUp(
        &self,
        rts: Option<&tablet_pc::IRealTimeStylus>,
        stylus_info: *const tablet_pc::StylusInfo,
        props_in_packet: u32,
        packet: *const i32,
        _: *mut *mut i32,
    ) -> WinResult<()> {
        self.panic_wrapper(AssertUnwindSafe(|| {
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
                        std::slice::from_raw_parts(
                            packet,
                            usize::try_from(props_in_packet).unwrap(),
                        )
                    }
                }
                // Spec says <= 32!
                _ => return Err(E_INVALIDARG.into()),
            };

            // This is not an optional field, according to spec.
            let stylus_info = unsafe { stylus_info.as_ref() }.ok_or(E_POINTER)?;
            let mut lock = self
                .shared_frame
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            // Delegate!
            // We don't have specific states for Up/Down transitions, those are handled
            // implicitly but observing previous state and new state.
            lock.handle_packets(&self.rts, *stylus_info, 1, props, StylusPhase::InAir);

            Ok(())
        }))
    }

    fn StylusButtonDown(
        &self,
        rts: Option<&tablet_pc::IRealTimeStylus>,
        sid: u32,
        button_guid: *const core::GUID,
        _stylus_position: *mut POINT,
    ) -> WinResult<()> {
        self.panic_wrapper(AssertUnwindSafe(|| {
            self.poison_bail()?;
            if rts != Some(&self.rts) {
                return Err(E_INVALIDARG.into());
            }
            if button_guid.is_null() {
                return Err(E_POINTER.into());
            }
            let button_guid = unsafe { *button_guid };

            let mut lock = self
                .shared_frame
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            let Some(tool) = lock.get_tool(sid) else {
                return Ok(());
            };

            let tool = *tool.internal_id.unwrap_ink();
            lock.events.push(crate::events::raw::Event::Tool {
                tool,
                event: crate::events::raw::ToolEvent::Button {
                    button_id: ButtonID(button_guid).into(),
                    pressed: true,
                },
            });

            Ok(())
        }))
    }

    fn StylusButtonUp(
        &self,
        rts: Option<&tablet_pc::IRealTimeStylus>,
        sid: u32,
        button_guid: *const core::GUID,
        _stylus_position: *mut POINT,
    ) -> WinResult<()> {
        self.panic_wrapper(AssertUnwindSafe(|| {
            self.poison_bail()?;
            if rts != Some(&self.rts) {
                return Err(E_INVALIDARG.into());
            }
            if button_guid.is_null() {
                return Err(E_POINTER.into());
            }
            let button_guid = unsafe { *button_guid };

            let mut lock = self
                .shared_frame
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            let Some(tool) = lock.get_tool(sid) else {
                return Ok(());
            };

            let tool = *tool.internal_id.unwrap_ink();
            lock.events.push(crate::events::raw::Event::Tool {
                tool,
                event: crate::events::raw::ToolEvent::Button {
                    button_id: ButtonID(button_guid).into(),
                    pressed: true,
                },
            });

            Ok(())
        }))
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
        self.panic_wrapper(AssertUnwindSafe(|| {
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
            let mut lock = self
                .shared_frame
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            // Delegate!
            lock.handle_packets(
                &self.rts,
                *stylus_info,
                num_packets,
                props,
                StylusPhase::InAir,
            );

            Ok(())
        }))
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
        self.panic_wrapper(AssertUnwindSafe(|| {
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
            let mut lock = self
                .shared_frame
                .lock()
                .unwrap_or_else(PoisonError::into_inner);

            // Delegate!
            lock.handle_packets(
                &self.rts,
                *stylus_info,
                num_packets,
                props,
                StylusPhase::Touched,
            );

            Ok(())
        }))
    }

    fn TabletAdded(
        &self,
        rts: Option<&tablet_pc::IRealTimeStylus>,
        tablet: Option<&tablet_pc::IInkTablet>,
    ) -> WinResult<()> {
        self.panic_wrapper(AssertUnwindSafe(|| {
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

            let mut lock = self
                .shared_frame
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            unsafe { lock.append_tablet(&self.rts, tablet, tcid) };

            poison.disarm();
            Ok(())
        }))
    }

    fn TabletRemoved(
        &self,
        _rts: Option<&tablet_pc::IRealTimeStylus>,
        tablet_idx: i32,
    ) -> WinResult<()> {
        self.panic_wrapper(AssertUnwindSafe(|| {
            // Poison section - we need to set our internal tablet array to match RTS or bad stuff happens.
            let poison = self.poison_on_drop()?;

            let mut lock = self
                .shared_frame
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            let _ = lock
                // IMPORTANT! use this method to handle all the bookkeeping. `tablet_idx`
                // is not a direct subscript into self.raw_tablets!
                .delete_tcid_by_idx(tablet_idx)
                // Out-of-bounds
                .map_err(|()| E_INVALIDARG)?;

            poison.disarm();
            Ok(())
        }))
    }

    fn UpdateMapping(&self, _: Option<&tablet_pc::IRealTimeStylus>) -> WinResult<()> {
        self.panic_wrapper(AssertUnwindSafe(|| {
            // Called on DPI change, need to re-fetch the conversion factor from HIMETRIC to logical pixels.
            self.poison_bail()?;
            let mut lock = self
                .shared_frame
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            lock.himetric_to_logical_pixel = unsafe { fetch_himetric_to_logical_pixel(lock.hwnd) };

            Ok(())
        }))
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
