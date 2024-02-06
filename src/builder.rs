use crate::{Backing, Manager, ManagerError};
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
    // Unimplementable on `rwh_05`, as it's safety conditions are not strong enough to ensure this
    // is sound!
    // Silly clippy, it's a self-describing err type!
    #[allow(clippy::missing_errors_doc)]
    pub fn build_shared(
        self,
        rwh: std::sync::Arc<impl raw_window_handle::HasDisplayHandle + 'static>,
    ) -> Result<Manager, ManagerError> {
        match rwh.display_handle()?.as_raw() {
            raw_window_handle::RawDisplayHandle::Wayland(wlh) => {
                // Erase the type, we don't care - we just need to be able to `Drop` it and to keep it around as
                // long as we need!
                let backing = rwh as std::sync::Arc<dyn crate::Erased>;
                // Safety - The returned `display_handle` is valid for as long as `rwh` is due to
                // safety bound on `DisplayHandle::borrow_raw`. Since we keep the `rwh` alive inside the manager,
                // the pointer is thus valid for the lifetime of the manager.
                Ok(unsafe {
                    Manager::build_wayland_display(
                        wlh.display.as_ptr().cast(),
                        Backing::Arc(backing),
                    )
                })
            }
            _ => Err(ManagerError::Unsupported),
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
    ) -> Result<Manager, ManagerError> {
        match rwh {
            raw_window_handle::RawDisplayHandle::Wayland(wlh) => {
                // Safety - deferred to this fn's contract
                Ok(unsafe {
                    Manager::build_wayland_display(wlh.display.as_ptr().cast(), Backing::Raw)
                })
            }
            _ => Err(ManagerError::Unsupported),
        }
    }
}
