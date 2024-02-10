use windows::core::{self, Result as WinResult};
use windows::Win32::Foundation::{HANDLE_PTR, POINT};
use windows::Win32::System::Com as com;
use windows::Win32::UI::TabletPC as tablet_pc;

#[windows::core::implement(tablet_pc::IStylusSyncPlugin)]
struct Plugin;
impl tablet_pc::IStylusSyncPlugin_Impl for Plugin {}
#[allow(non_snake_case)]
// FIXME: Go through and rename to something more rust-y.
#[allow(unused_variables)]
#[allow(clippy::similar_names)]
impl tablet_pc::IStylusPlugin_Impl for Plugin {
    fn RealTimeStylusEnabled(
        &self,
        pirtssrc: Option<&tablet_pc::IRealTimeStylus>,
        ctcidcount: u32,
        ptcids: *const u32,
    ) -> WinResult<()> {
        todo!()
    }

    fn RealTimeStylusDisabled(
        &self,
        pirtssrc: Option<&tablet_pc::IRealTimeStylus>,
        ctcidcount: u32,
        ptcids: *const u32,
    ) -> WinResult<()> {
        todo!()
    }

    fn StylusInRange(
        &self,
        pirtssrc: Option<&tablet_pc::IRealTimeStylus>,
        tcid: u32,
        sid: u32,
    ) -> WinResult<()> {
        todo!()
    }

    fn StylusOutOfRange(
        &self,
        pirtssrc: Option<&tablet_pc::IRealTimeStylus>,
        tcid: u32,
        sid: u32,
    ) -> WinResult<()> {
        todo!()
    }

    fn StylusDown(
        &self,
        pirtssrc: Option<&tablet_pc::IRealTimeStylus>,
        pstylusinfo: *const tablet_pc::StylusInfo,
        cpropcountperpkt: u32,
        ppacket: *const i32,
        ppinoutpkt: *mut *mut i32,
    ) -> WinResult<()> {
        todo!()
    }

    fn StylusUp(
        &self,
        pirtssrc: Option<&tablet_pc::IRealTimeStylus>,
        pstylusinfo: *const tablet_pc::StylusInfo,
        cpropcountperpkt: u32,
        ppacket: *const i32,
        ppinoutpkt: *mut *mut i32,
    ) -> WinResult<()> {
        todo!()
    }

    fn StylusButtonDown(
        &self,
        pirtssrc: Option<&tablet_pc::IRealTimeStylus>,
        sid: u32,
        pguidstylusbutton: *const core::GUID,
        pstyluspos: *mut POINT,
    ) -> WinResult<()> {
        todo!()
    }

    fn StylusButtonUp(
        &self,
        pirtssrc: Option<&tablet_pc::IRealTimeStylus>,
        sid: u32,
        pguidstylusbutton: *const core::GUID,
        pstyluspos: *mut POINT,
    ) -> WinResult<()> {
        todo!()
    }

    fn InAirPackets(
        &self,
        pirtssrc: Option<&tablet_pc::IRealTimeStylus>,
        pstylusinfo: *const tablet_pc::StylusInfo,
        cpktcount: u32,
        cpktbufflength: u32,
        ppackets: *const i32,
        pcinoutpkts: *mut u32,
        ppinoutpkts: *mut *mut i32,
    ) -> WinResult<()> {
        todo!()
    }

    fn Packets(
        &self,
        pirtssrc: Option<&tablet_pc::IRealTimeStylus>,
        pstylusinfo: *const tablet_pc::StylusInfo,
        cpktcount: u32,
        cpktbufflength: u32,
        ppackets: *const i32,
        pcinoutpkts: *mut u32,
        ppinoutpkts: *mut *mut i32,
    ) -> WinResult<()> {
        todo!()
    }

    fn CustomStylusDataAdded(
        &self,
        pirtssrc: Option<&tablet_pc::IRealTimeStylus>,
        pguidid: *const core::GUID,
        cbdata: u32,
        pbdata: *const u8,
    ) -> WinResult<()> {
        todo!()
    }

    fn SystemEvent(
        &self,
        pirtssrc: Option<&tablet_pc::IRealTimeStylus>,
        tcid: u32,
        sid: u32,
        event: u16,
        eventdata: &tablet_pc::SYSTEM_EVENT_DATA,
    ) -> WinResult<()> {
        todo!()
    }

    fn TabletAdded(
        &self,
        pirtssrc: Option<&tablet_pc::IRealTimeStylus>,
        pitablet: Option<&tablet_pc::IInkTablet>,
    ) -> WinResult<()> {
        todo!()
    }

    fn TabletRemoved(
        &self,
        pirtssrc: Option<&tablet_pc::IRealTimeStylus>,
        itabletindex: i32,
    ) -> WinResult<()> {
        todo!()
    }

    fn Error(
        &self,
        pirtssrc: Option<&tablet_pc::IRealTimeStylus>,
        piplugin: Option<&tablet_pc::IStylusPlugin>,
        datainterest: tablet_pc::RealTimeStylusDataInterest,
        hrerrorcode: core::HRESULT,
        lptrkey: *mut isize,
    ) -> WinResult<()> {
        todo!()
    }

    fn UpdateMapping(&self, pirtssrc: Option<&tablet_pc::IRealTimeStylus>) -> WinResult<()> {
        todo!()
    }

    fn DataInterest(&self) -> WinResult<tablet_pc::RealTimeStylusDataInterest> {
        todo!()
    }
}

pub struct Manager {
    _rts: tablet_pc::IRealTimeStylus,
    _marshaler: core::IUnknown,
    _plugin: tablet_pc::IStylusSyncPlugin,
}
impl Manager {
    /// Creates a tablet manager with from the given `HWND`.
    /// # Safety
    /// The given `HWND` must be valid as long as the returned `Manager` is alive.
    pub(crate) unsafe fn build_hwnd(hwnd: std::num::NonZeroIsize) -> Self {
        let rts = unsafe {
            // Safety: Uhh..
            let rts: tablet_pc::IRealTimeStylus = com::CoCreateInstance(
                std::ptr::from_ref(&tablet_pc::RealTimeStylus),
                None,
                com::CLSCTX_ALL,
            )
            .unwrap();
            // Bitwise cast from isize to usize. why integers. why.
            let hwnd = HANDLE_PTR(std::mem::transmute::<isize, usize>(hwnd.get()));
            // Safety: Must survive as long as `rts`. deferred to this fn's contract.
            rts.SetHWND(hwnd).unwrap();
            rts
        };
        let index = unsafe { rts.GetStylusSyncPluginCount() }.unwrap();
        let plugin = tablet_pc::IStylusSyncPlugin::from(Plugin);
        //wtf is a marshaler D:
        let marshaler = unsafe { com::CoCreateFreeThreadedMarshaler(&plugin) }.unwrap();

        // Err: INVALID_ARG
        unsafe { rts.AddStylusSyncPlugin(index, &plugin) }.unwrap();

        Self {
            _rts: rts,
            _marshaler: marshaler,
            _plugin: plugin,
        }
    }
}
impl super::PlatformImpl for Manager {
    #[allow(clippy::missing_errors_doc)]
    fn pump(&mut self) -> Result<(), crate::PumpError> {
        //self.queue.dispatch_pending(&mut self.state)?;
        todo!()
    }
    #[must_use]
    fn timestamp_granularity(&self) -> Option<std::time::Duration> {
        // Wayland always reports, and with millisecond granularity.
        None
    }
    #[must_use]
    fn pads(&self) -> &[crate::pad::Pad] {
        // Ink doesn't report any of these capabilities or events :<
        &[]
    }
    #[must_use]
    fn tools(&self) -> &[crate::tool::Tool] {
        todo!()
    }
    #[must_use]
    fn tablets(&self) -> &[crate::tablet::Tablet] {
        todo!()
    }
    #[must_use]
    fn make_summary(&self) -> crate::events::summary::Summary {
        todo!()
    }
}
