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
//! To get started, create a [`Builder`].
//!
//! ## Hardware support
//! Aims to support a wide range of devices, as long as they use standard platform APIs (ie, no *Wacom SDK*).
//!
//! During development, tested on:
//! * *Wacom Cintiq 16* \[DTK-1660\]
//! * *Wacom Intuos (S)* \[CTL-4100\]
//! * *Wacom Intuos Pro small* \[PTH-451\]
//! * *Wacom Pro Pen 2*
//! * *Wacom Pro Pen 2k*
//! * *XP-Pen Deco-01*
//!
//! **Note:** Nearly every graphics tablet/driver has no shortage of quirks. Some platforms try to correct for that,
//! some dont. While API-level quirks are smoothed out by this crate, per-device quirk correction is beyond the scope
//! of this project and quirked values are reported as-is. Where possible, documentation notes are included to warn
//! where quirks are known to occur. If you encounter additional quirks, please submit a PR documenting them.
//! **Guarantees are made only when explicitly stated so!**
//!
//! ## Examples
//! See [the examples directory](https://github.com/Fuzzyzilla/wl-tablet/tree/master/examples) for how
//! this can be integrated into `winit` or `eframe` projects.

#![warn(clippy::pedantic)]
#![forbid(unsafe_op_in_unsafe_fn)]
pub mod builder;
pub mod events;
pub mod pad;
pub mod tablet;
pub mod tool;
mod wl;
pub use builder::Builder;
use events::Events;
use wayland_client::DispatchError;

pub enum TabletEvent {}

/// A trait that every object is.
/// Used to cast things to `dyn Erased` which leaves us with a wholly erased type.
trait Erased {}
impl<T> Erased for T {}

#[allow(dead_code)]
enum Backing {
    // The RWH owner is arc'd and thus is valid for as long as we need.
    // We don't need to remember what type the pointer came from, but `dyn WhateverTrait` remembers
    // enough to be able to destruct it cleanly.
    Arc(std::sync::Arc<dyn Erased>),
    // The RWH is some kind of raw pointer without lifetimes, and the user has guaranteed
    // that it is valid as long as this object lives.
    Raw,
}

#[derive(thiserror::Error, Debug)]
pub enum ManagerError {
    /// The given window handle doesn't use a supported connection type.
    #[error("handle doesn't contain a supported display type")]
    Unsupported,
    /// Failed to acquire a window handle
    #[error(transparent)]
    HandleError(#[from] raw_window_handle::HandleError),
}
#[derive(Clone, Copy, Debug)]
pub enum Backend {
    /// [https://wayland.app/protocols/tablet-unstable-v2]
    WaylandTabletUnstableV2,
    // /// [https://learn.microsoft.com/en-us/windows/win32/api/msinkaut/] [https://learn.microsoft.com/en-us/windows/win32/tablet/realtimestylus-reference]
    // /// [https://learn.microsoft.com/en-us/windows/win32/tablet/packetpropertyguids-constants]
    // WindowsInk,
}

/// Manages a connection to the OS's tablet server. This is the main
/// entry point for enumerating hardware and listening for events.
pub struct Manager {
    _display: wayland_client::protocol::wl_display::WlDisplay,
    _conn: wayland_client::Connection,
    queue: wayland_client::EventQueue<wl::TabletState>,
    _qh: wayland_client::QueueHandle<wl::TabletState>,
    state: wl::TabletState,

    // `_backing` MUST BE LAST IN DECLARATION ORDER!
    // the other fields may rely on the lifetime guarantees granted by the contents
    // of this `Backing`, and it's guaranteed that drop order == declaration order.
    // Never thought I'd end up in a situation like this in Rust :P
    _backing: Backing,
}
impl Manager {
    /// Creates a tablet manager with from the given pointer to `wl_display`.
    /// # Safety
    /// The given display pointer must be valid as long as the returned `Manager` is alive. The [`Backing`] parameter
    /// is kept alive with the returned Manager, which can be used to uphold this requirement.
    pub(crate) unsafe fn build_wayland_display(wl_display: *mut (), backing: Backing) -> Manager {
        // Safety - deferred to this fn's contract
        let backend =
            unsafe { wayland_backend::client::Backend::from_foreign_display(wl_display.cast()) };
        let conn = wayland_client::Connection::from_backend(backend);
        let display = conn.display();
        let queue = conn.new_event_queue::<crate::wl::TabletState>();
        let qh = queue.handle();
        // Allow the manager impl to sift through and capture extention handles
        display.get_registry(&qh, ());
        Manager {
            _backing: backing,
            _display: display,
            _conn: conn,
            queue,
            _qh: qh,
            state: crate::wl::TabletState::default(),
        }
    }
}
impl Manager {
    /// Parse pending events. Will update the hardware reports, as well as collecting inputs.
    /// Assumes another client on this connection is performing reads, which will be the case
    /// if you're using `winit` or `eframe`.
    #[allow(clippy::missing_errors_doc)]
    pub fn pump(&mut self) -> Result<Events<'_>, DispatchError> {
        let _events = self.queue.dispatch_pending(&mut self.state)?;
        Ok(Events { manager: &*self })
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
    /// Query the API currently in use. May give some hints as to the capabilities and limitations.
    #[must_use]
    pub fn backed(&self) -> Backend {
        Backend::WaylandTabletUnstableV2
    }
    /// Access pad information. Pads are the physical object that you draw on,
    /// and may have touch support, an inbuilt display, lights, buttons, rings, and/or sliders.
    ///
    /// Pads are ordered arbitrarily.
    #[must_use]
    pub fn pads(&self) -> &[pad::Pad] {
        &self.state.pads.finished
    }
    /// Access tool information. Tools are styluses or other hardware that
    /// communicate with one or more pads, and are responsible for reporting movements, pressure, etc.,
    /// and may have multiple buttons.
    ///
    /// Tools are ordered arbitrarily.
    #[must_use]
    pub fn tools(&self) -> &[tool::Tool] {
        &self.state.tools.finished
    }
    /// A tablet is the entry point for interactive devices, and the top level of the hierarchy
    /// which may expose several pads or tools.
    ///
    /// Tablets are ordered arbitrarily.
    #[must_use]
    pub fn tablets(&self) -> &[tablet::Tablet] {
        &self.state.tablets.finished
    }
    #[must_use]
    fn make_summary(&self) -> events::summary::Summary {
        let try_summarize = || -> Option<events::summary::Summary> {
            let sum = self.state.summary.clone()?;

            let tablet = self
                .tablets()
                .iter()
                .find(|tab| tab.obj_id == sum.tablet_id)?;
            let tool = self.tools().iter().find(|tab| tab.obj_id == sum.tool_id)?;
            Some(events::summary::Summary {
                tool: events::summary::ToolState::In(events::summary::InState {
                    tablet,
                    tool,
                    pose: sum.pose,
                    down: sum.down,
                    timestamp: Some(sum.time),
                }),
                pads: &[],
            })
        };

        // try block pls..
        try_summarize().unwrap_or(events::summary::Summary {
            tool: events::summary::ToolState::Out,
            pads: &[],
        })
    }
}
