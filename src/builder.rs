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
    #[error("{0:?}")]
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
    pub fn build_shared<Holder>(self, rwh: &std::sync::Arc<Holder>) -> Result<Manager, BuildError>
    where
        Holder: raw_window_handle::HasDisplayHandle + raw_window_handle::HasWindowHandle + 'static,
    {
        // Unsize - erase the type, we don't care during runtime. We just need to be able to `Drop` it and to keep it around as
        // long as we need!
        // We can *kind of* skip the clone using an identity `transmute` to extent it's life - the ref remains valid even after unsizing.
        // butttttt the safety is nuanced and may actually be instantaneous UB at the end of scope if `build` returns `Err`. Defeated by borrowchk again!
        let backing = Backing::Arc(rwh.clone() as _);
        // Safety - The returned `display_handle` is valid for as long as `rwh` is due to
        // safety bound on `DisplayHandle::borrow_raw`. Since we keep the `rwh` alive inside the manager,
        // the pointer is thus valid for the lifetime of the manager.
        unsafe { self.build(rwh as &Holder, backing) }
    }
    /// Build from a display handle carrier, such as a reference to a `winit` window, with unbound lifetime.
    ///
    /// # Safety
    /// The given display handle carrier must be keep the window and display pointers valid as long as the returned `Manager` is alive.
    ///
    /// ***`rwh` is dropped at the end of scope** - not kept alive within the `Manager` - thus cannot be used to ensure safety!*
    // Silly clippy, it's a self-describing err type!
    #[allow(clippy::missing_errors_doc)]
    pub unsafe fn build_raw(
        self,
        rwh: impl raw_window_handle::HasDisplayHandle + raw_window_handle::HasWindowHandle,
    ) -> Result<Manager, BuildError> {
        // Safety: forwarded to this fn's contract.
        unsafe { self.build(rwh, Backing::Raw) }
    }
    /// Private, raw builder that the others delegate into.
    ///
    /// The `rwh` implementor object is *not* kept.
    /// # Safety
    /// The given display handle carrier must be keep the window and display pointers valid as long as the returned `Manager` is alive.
    /// This may be insured by using the `Backing` parameter which will be kept alive for as long as the returned Manager is.
    unsafe fn build(
        self,
        rwh: impl raw_window_handle::HasDisplayHandle + raw_window_handle::HasWindowHandle,
        backing: Backing,
    ) -> Result<Manager, BuildError> {
        let internal = match rwh.display_handle()?.as_raw() {
            #[cfg(wl_tablet)]
            raw_window_handle::RawDisplayHandle::Wayland(wlh) => {
                Ok(crate::platform::PlatformManager::Wayland(
                    // Safety: forwarded to this fn's contract.
                    unsafe {
                        crate::platform::wl::Manager::build_wayland_display(
                            wlh.display.as_ptr().cast(),
                        )
                    },
                ))
            }
            #[cfg(ink_rts)]
            raw_window_handle::RawDisplayHandle::Windows(_) => {
                // We need the window handle for this :V
                // Notably, WinRT is unsupported - It doesn't have the IRealTimeStylus API at all.
                if let raw_window_handle::RawWindowHandle::Win32(wh) = rwh.window_handle()?.as_raw()
                {
                    Ok(crate::platform::PlatformManager::Ink(
                        // Safety: forwarded to this fn's contract.
                        // Fixme: unwrap.
                        unsafe { crate::platform::ink::Manager::build_hwnd(wh.hwnd).unwrap() },
                    ))
                } else {
                    Err(BuildError::Unsupported)
                }
            }
            _ => Err(BuildError::Unsupported),
        }?;

        // Dummy!
        let Self {} = self;

        Ok(Manager {
            internal,
            _backing: backing,
        })
    }
}
