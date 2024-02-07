//! Builder-style configuration for connecting to the system tablet API.
//!
//! For a default configuration, `Builder::new().build_{shared, raw}` is all you need!

use crate::{Backing, Manager};

#[derive(thiserror::Error, Debug)]
pub enum BuildError {
    /// The given window handle doesn't use a supported connection type.
    /// This includes cases where the platform is otherwise supported but the feature was disabled at compile-time.
    #[error("handle doesn't contain a supported display type")]
    Unsupported,
    /// Failed to acquire a window handle
    #[error("{:?}", .0)]
    HandleError(raw_window_handle::HandleError),
}
// #[from] thiserror attribute breaks horribly D:
impl From<raw_window_handle::HandleError> for BuildError {
    fn from(value: raw_window_handle::HandleError) -> Self {
        Self::HandleError(value)
    }
}

/// Pre-construction configuration for a [`Manager`].
#[derive(Default)]
pub struct Builder {}

/// # Configuration
impl Builder {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}
/// # Finishing
impl Builder {
    /// Build from a shared display handle carrier. Internally, this `Arc` is kept alive for as
    /// long as the returned `Manager` is around ensuring safe operation.
    // Unimplementable on `rwh_05`, as its safety conditions are not strong enough to ensure this
    // is sound!
    // Silly clippy, it's a self-describing err type!
    #[allow(clippy::missing_errors_doc)]
    pub fn build_shared(
        self,
        rwh: std::sync::Arc<impl raw_window_handle::HasDisplayHandle + 'static>,
    ) -> Result<Manager, BuildError> {
        match rwh.display_handle()?.as_raw() {
            #[cfg(wl_tablet)]
            raw_window_handle::RawDisplayHandle::Wayland(wlh) => {
                // Erase the type, we don't care - we just need to be able to `Drop` it and to keep it around as
                // long as we need!
                let backing = Backing::Arc(rwh as _);
                // Safety - The returned `display_handle` is valid for as long as `rwh` is due to
                // safety bound on `DisplayHandle::borrow_raw`. Since we keep the `rwh` alive inside the manager,
                // the pointer is thus valid for the lifetime of the manager.
                let internal = crate::platform::PlatformManager::Wayland(unsafe {
                    // Safety - deferred to this fn's contract
                    crate::platform::wl::Manager::build_wayland_display(wlh.display.as_ptr().cast())
                });
                Ok(Manager {
                    internal,
                    _backing: backing,
                })
            }
            _ => Err(BuildError::Unsupported),
        }
    }
    /// Build from a display handle carrier, such as a reference to a `winit` window, with unbound lifetime.
    /// # Safety
    /// The given display handle must be valid as long as the returned `Manager` is alive.
    // Weirdly, this code is fine if neither rwh are enabled, since `AnyDisplayVersion` becomes
    // uninhabited and it essentially becomes `match ! {}` which is always valid. Wowie!
    // Silly clippy, it's a self-describing err type!
    #[allow(clippy::missing_errors_doc)]
    pub unsafe fn build_raw(
        self,
        rwh: raw_window_handle::RawDisplayHandle,
    ) -> Result<Manager, BuildError> {
        match rwh {
            #[cfg(wl_tablet)]
            raw_window_handle::RawDisplayHandle::Wayland(wlh) => {
                let backing = Backing::Raw;
                let internal = crate::platform::PlatformManager::Wayland(unsafe {
                    // Safety - deferred to this fn's contract
                    crate::platform::wl::Manager::build_wayland_display(wlh.display.as_ptr().cast())
                });
                Ok(Manager {
                    internal,
                    _backing: backing,
                })
            }
            _ => Err(BuildError::Unsupported),
        }
    }
}
