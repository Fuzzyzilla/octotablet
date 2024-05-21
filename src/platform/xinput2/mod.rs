pub type ID = ();
pub type ButtonID = ();

pub struct Manager;

impl Manager {
    pub unsafe fn build_window(_opts: crate::Builder, _window: u64) -> Self {
        Self
    }
}

impl super::PlatformImpl for Manager {
    fn pads(&self) -> &[crate::pad::Pad] {
        &[]
    }
    fn pump(&mut self) -> Result<(), crate::PumpError> {
        Ok(())
    }
    fn raw_events(&self) -> super::RawEventsIter<'_> {
        super::RawEventsIter::XInput2([].iter())
    }
    fn tablets(&self) -> &[crate::tablet::Tablet] {
        &[]
    }
    fn timestamp_granularity(&self) -> Option<std::time::Duration> {
        None
    }
    fn tools(&self) -> &[crate::tool::Tool] {
        &[]
    }
}
