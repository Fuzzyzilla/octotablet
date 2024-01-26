//! # ~~Cross-platform~~\* Tablet, Pad, and Stylus interface
//! *\*Only wayland support right now lol*
//!
//! Current plans are Wayland's *`tablet_unstable_v2`*, Windows Ink, and X11's *`xinput`*, but aims to
//! one day provide a unified API for tablet access across Linux, Windows, and Mac. Additionally,
//! rather than providing the intersection of these platforms' capabilities, aims to expose the union of them
//! such that no features are lost to abstraction.
//!
//! This library requires low-level access to the windowing server, which is provided
//! by many windowing abstractions through the [`raw_window_handle`](https://crates.io/crates/raw-window-handle) crate.
//!
//! To get started, create a [`Manager`].
//!
//! ## Examples
//! See [the examples directory](https://github.com/Fuzzyzilla/wl-tablet/tree/master/examples) for how
//! this can be integrated into `winit` or `eframe` projects.

#![warn(clippy::pedantic)]
#![deny(unsafe_op_in_unsafe_fn)]
pub mod events;
pub mod pad;
pub mod tablet;
pub mod tool;
mod wl;
use events::{Event, EventIterator};
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
    /// The given window handle doesn't use a supported connection type.
    #[error("handle doesn't contain a supported display type")]
    Unsupported,
}

/// Manages a connection to the OS's tablet server. This is the main
/// entry point for enumerating hardware and listening for events.
pub struct Manager {
    _display_owner: Backing,
    _display: wayland_client::protocol::wl_display::WlDisplay,
    _conn: wayland_client::Connection,
    queue: wayland_client::EventQueue<wl::TabletState>,
    _qh: wayland_client::QueueHandle<wl::TabletState>,
    state: wl::TabletState,
}
/// # Constructors
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
}
impl Manager {
    /// Parse pending events. Will update the hardware reports, as well as collecting inputs.
    /// Assumes another client on this connection is performing reads, which will be the case
    /// if you're using `winit` or `eframe`.
    //#[must_use]
    #[allow(clippy::missing_errors_doc)]
    pub fn pump(&mut self) -> Result<impl Iterator<Item = Event<'_>> + '_, DispatchError> {
        let _events = self.queue.dispatch_pending(&mut self.state)?;
        Ok(EventIterator { _manager: &*self })
    }
    /// Query the precision of [timestamps](events::FrameTimestamp) provided along with axis events, if any.
    /// This does *not* represent the polling rate. `None` if timestamps are not collected.
    ///
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
    pub fn pads(&self) -> &[pad::Pad] {
        &self.state.pads.collection.finished
    }
    /// Access tool information. Tools are styluses or other hardware that
    /// communicate with one or more pads, and are responsible for reporting movements, pressure, etc.,
    /// and may have multiple buttons.
    ///
    /// Returned tools are in order they were discovered, and are never removed.
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
