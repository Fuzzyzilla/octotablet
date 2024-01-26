#![warn(clippy::pedantic)]
#![deny(unsafe_op_in_unsafe_fn)]
pub mod tablet;
pub mod tool;
mod wl;
use wayland_client::DispatchError;

pub enum TabletEvent {}

#[allow(dead_code)]
enum Backing {
    // The RWH owner is boxed and thus is valid for as long as we need.
    Box(Box<dyn std::any::Any>),
    // The RWH owner is arc'd and thus is valid for as long as we need.
    Arc(std::sync::Arc<dyn std::any::Any>),
    // The RWH is a pointer, and the user has guaranteed that it is valid as long as this object lives.
    Raw,
}

#[derive(thiserror::Error, Debug)]
pub enum ManagerError {
    #[error("handle doesn't contain a supported display type")]
    Unsupported,
}

pub struct Manager {
    _display_owner: Backing,
    _display: wayland_client::protocol::wl_display::WlDisplay,
    _conn: wayland_client::Connection,
    queue: wayland_client::EventQueue<wl::TabletState>,
    _qh: wayland_client::QueueHandle<wl::TabletState>,
    state: wl::TabletState,
}
impl Manager {
    /// Creates a tablet manager with a disconnected lifetime from the given display.
    /// # Safety
    /// The given display handle must be valid as long as the returned `Manager` is alive.
    // Silly clippy, it's a self-describing err type!
    #[allow(clippy::missing_errors_doc)]
    pub unsafe fn new_raw(
        handle: raw_window_handle::RawDisplayHandle,
    ) -> Result<Self, ManagerError> {
        match handle {
            raw_window_handle::RawDisplayHandle::Wayland(wlh) => {
                // Safety - deferred to this fn's contract
                Ok(unsafe { Self::from_wayland_display(wlh.display.cast()) })
            }
            _ => Err(ManagerError::Unsupported),
        }
    }
    /// Creates a tablet manager with a disconnected lifetime from the given pointer to `wl_display`.
    /// # Safety
    /// The given display pointer must be valid as long as the returned `Manager` is alive.
    pub unsafe fn from_wayland_display(wl_display: *mut ()) -> Self {
        // Safety - deferred to this fn's contract
        let backend =
            unsafe { wayland_backend::client::Backend::from_foreign_display(wl_display.cast()) };
        let conn = wayland_client::Connection::from_backend(backend);
        let display = conn.display();
        let queue = conn.new_event_queue::<wl::TabletState>();
        let qh = queue.handle();
        // Allow the manager impl to sift through and capture extention handles
        display.get_registry(&qh, ());
        Self {
            _display_owner: Backing::Raw,
            _display: display,
            _conn: conn,
            queue,
            _qh: qh,
            state: wl::TabletState::default(),
        }
    }
    /// Parse pending events. Will update the hardware reports, as well as collecting inputs.
    /// Assumes another client on this connection is performing reads.
    //#[must_use]
    #[allow(clippy::missing_errors_doc)]
    pub fn pump(&mut self) -> Result<(), DispatchError> {
        let _events = self.queue.dispatch_pending(&mut self.state)?;
        Ok(())
    }
    /// Query the precision of timestamps provided along with axis events, if any.
    /// `None` if timestamps are not collected.
    /// This is useful for understanding velocities even if events aren't consumed immediately.
    #[must_use]
    pub fn timestamp_resolution(&self) -> Option<std::time::Duration> {
        // Wayland always reports, and with millisecond granularity.
        Some(std::time::Duration::from_millis(1))
    }
    /// Access pad information. Pads are the physical object that you draw on,
    /// and may have touch support, an inbuilt display, lights, buttons, rings, and/or sliders.
    ///
    /// Pads are ordered arbitrarily.
    #[must_use]
    pub fn pads(&self) -> &[()] {
        &[]
    }
    /// Access tool information. Tools are styluses or other hardware that
    /// communicate with one or more pads, and are responsible for reporting movements, pressure, etc.,
    /// and may have multiple buttons.
    ///
    /// Tools are ordered arbitrarily.
    #[must_use]
    pub fn tools(&self) -> &[tool::Tool] {
        &self.state.tools.collection.finished
    }
    /// A tablet is the entry point for interactive devices, and the top level of the hierarchy
    /// which may expose several pads or tools.
    ///
    /// Tablets are ordered arbitrarily.
    #[must_use]
    pub fn tablets(&self) -> &[tablet::Tablet] {
        &self.state.tablets.collection.finished
    }
}
