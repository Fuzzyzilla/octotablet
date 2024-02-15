//! # Cross-platform [Tablet](tablet), [Pad](pad), and [Stylus](tool) API 🐙✨
//!
//! Current plans are Wayland's *`tablet_unstable_v2`*, Windows Ink's `RealTimeStylus`, and X11's *`xinput`*, but aims to
//! provide a unified API for tablet access across more platforms in the future. Additionally, rather than providing the intersection
//! of these platforms' capabilities, it aims to expose the union of them such that no features are lost to abstraction.
//!
//! This library requires low-level access to the windowing server, which is provided
//! by many windowing abstractions through the [`raw_window_handle`](https://crates.io/crates/raw-window-handle) crate.
//!
//! To get started, create a [`Builder`].
//!
//! ## Hardware support
//! Aims to support a wide range of devices, as long as they use standard platform APIs (ie, no tablet-specific APIs).
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
//! See [the examples directory](https://github.com/Fuzzyzilla/octotablet/tree/master/examples) for how
//! this can be integrated into `winit` or `eframe` projects.

#![warn(clippy::pedantic)]
#![forbid(unsafe_op_in_unsafe_fn)]

pub mod builder;
pub mod events;
pub mod pad;
mod platform;
use platform::InternalID;
pub mod tablet;
pub mod tool;
pub use builder::Builder;
use events::Events;
use platform::PlatformImpl;

pub(crate) mod macro_bits {
    /// Implements an public opaque ID,
    /// assuming the struct has a `internal_id` which implements `Into<platform::InternalID>`
    macro_rules! impl_get_id {
        ($id_name:ident for $impl_for:ident) => {
            /// An opaque ID. Can be used to keep track of hardware, but only during its lifetime.
            /// Once the hardware is `Removed`, the ID loses meaning.
            #[derive(Clone, Debug, Hash, PartialEq, Eq)]
            #[allow(clippy::module_name_repetitions)]
            pub struct $id_name(crate::platform::InternalID);

            impl $impl_for {
                /// Opaque, transient ID of this tool, assigned arbitrarily by the software. Will not
                /// be stable across invocations or even unplugs/replugs!
                #[must_use]
                pub fn id(&self) -> $id_name {
                    $id_name(self.internal_id.clone().into())
                }
            }
        };
    }
    // Weird hacks to allow use from submodules..
    pub(crate) use impl_get_id;
}

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
#[derive(Clone, Copy, Debug)]
pub enum Backend {
    /// [`tablet_unstable_v2`](https://wayland.app/protocols/tablet-unstable-v2)
    ///
    /// **Note**: "unstable" here refers to the protocol itself, not to the stability of its integration into this crate!
    #[cfg(wl_tablet)]
    WaylandTabletUnstableV2,
    /// [`RealTimeStylus`](https://learn.microsoft.com/en-us/windows/win32/tablet/realtimestylus-reference)
    // [https://learn.microsoft.com/en-us/windows/win32/api/msinkaut/]
    // [https://learn.microsoft.com/en-us/windows/win32/tablet/packetpropertyguids-constants]
    #[cfg(ink_rts)]
    WindowsInkRealTimeStylus,
}
/// Errors that may occur during even pumping.
#[derive(thiserror::Error, Debug)]
pub enum PumpError {
    #[cfg(wl_tablet)]
    #[error(transparent)]
    WaylandDispatch(#[from] wayland_client::DispatchError),
}

/// Manages a connection to the OS's tablet server. This is the main
/// entry point for enumerating hardware and listening for events.
pub struct Manager {
    pub(crate) internal: platform::PlatformManager,
    // `_backing` MUST BE LAST IN DECLARATION ORDER!
    // the other fields may rely on the lifetime guarantees granted by the contents
    // of this `Backing`, and it's guaranteed that drop order == declaration order.
    // Never thought I'd end up in a situation like this in Rust :P
    pub(crate) _backing: Backing,
}
impl Manager {
    /// Parse pending events. Will update the hardware reports, as well as collecting inputs.
    /// Assumes another client on this connection is performing reads, which will be the case
    /// if you're using `winit` or `eframe`.
    #[allow(clippy::missing_errors_doc)]
    pub fn pump(&mut self) -> Result<Events<'_>, PumpError> {
        self.internal.pump()?;
        Ok(Events { manager: &*self })
    }
    /// Query the precision of [timestamps](events::FrameTimestamp) provided along with axis events, if any.
    /// This does *not* represent the polling rate. `None` if timestamps are not collected.
    ///
    /// This is useful for understanding velocities even if events aren't consumed immediately.
    #[must_use]
    pub fn timestamp_granularity(&self) -> Option<std::time::Duration> {
        self.internal.timestamp_granularity()
    }
    /// Query the API currently in use. May give some hints as to the capabilities and limitations.
    #[must_use]
    pub fn backed(&self) -> Backend {
        match self.internal {
            #[cfg(wl_tablet)]
            platform::PlatformManager::Wayland(_) => Backend::WaylandTabletUnstableV2,
            #[cfg(ink_rts)]
            platform::PlatformManager::Ink(_) => Backend::WindowsInkRealTimeStylus,
        }
    }
    /// Access pad information. Pads are the physical object that you draw on,
    /// and may have touch support, an inbuilt display, lights, buttons, rings, and/or sliders.
    ///
    /// Pads are ordered arbitrarily.
    #[must_use]
    pub fn pads(&self) -> &[pad::Pad] {
        self.internal.pads()
    }
    /// Access tool information. Tools are styluses or other hardware that
    /// communicate with one or more pads, and are responsible for reporting movements, pressure, etc.,
    /// and may have multiple buttons.
    ///
    /// Tools are ordered arbitrarily.
    #[must_use]
    pub fn tools(&self) -> &[tool::Tool] {
        self.internal.tools()
    }
    /// A tablet is the entry point for interactive devices, and the top level of the hierarchy
    /// which may expose several pads or tools.
    ///
    /// Tablets are ordered arbitrarily.
    #[must_use]
    pub fn tablets(&self) -> &[tablet::Tablet] {
        self.internal.tablets()
    }
}
