// Conditionally include each backend...
#[cfg(ink_rts)]
pub(crate) mod ink;
#[cfg(wl_tablet)]
pub(crate) mod wl;

/// Holds any one of the internal platform IDs.
/// Since these are always sealed away as an implementation detail, we can always
/// assume they're the right type since they can never be moved between `Manager`s.
// (because of this it could actually be a union. hmmmm...)
#[derive(Clone, Hash, Eq, PartialEq)]
pub(crate) enum InternalID {
    #[cfg(wl_tablet)]
    Wayland(wayland_backend::client::ObjectId),
    #[cfg(ink_rts)]
    Ink(u32),
}
impl std::fmt::Debug for InternalID {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        use std::hash::{Hash, Hasher};
        // We display as a hash since it's an opaque object, but we still want visual distinction between
        // differing IDs.
        // We *really* don't care what the results are here, as long as it's consistent during a single run.
        // Rather than pull in a dep, just use a random hasher from std!
        let mut h = std::collections::hash_map::DefaultHasher::new();
        self.hash(&mut h);
        f.debug_tuple("InternalID").field(&h.finish()).finish()
    }
}

/// Unwrappers. Impls are free to assume their IDs are always the right type, as there are no accessors
/// and no way to share IDs between managers of different backends.
///
/// In most (all?) compilation environments, these are infallible and compile down to nothing, hence the inline (profile me :3).
/// Regardless, the failure case is a logic error on the part of the implementation and should never be reached in practice.
impl InternalID {
    // Move formatting and unwinding machinery out of the inline path.
    // Tested on compiler explorer, this being inline(never) does *not* prevent it from being elided entirely
    // in the common case where it's dead code.
    #[cold]
    #[inline(never)]
    fn unwrap_failure() -> ! {
        panic!("Unwrap called on incorrect ID type")
    }
    #[cfg(wl_tablet)]
    #[inline]
    pub(crate) fn unwrap_wl(&self) -> &wayland_backend::client::ObjectId {
        #[allow(unreachable_patterns)]
        #[allow(clippy::match_wildcard_for_single_variants)]
        match self {
            Self::Wayland(id) => id,
            _ => Self::unwrap_failure(),
        }
    }
    #[cfg(ink_rts)]
    #[inline]
    pub(crate) fn unwrap_ink(&self) -> &u32 {
        #[allow(unreachable_patterns)]
        #[allow(clippy::match_wildcard_for_single_variants)]
        match self {
            Self::Ink(id) => id,
            _ => Self::unwrap_failure(),
        }
    }
}

/// Trait that all platforms implement, giving the main `Manager` higher-level access to the black box.
#[enum_dispatch::enum_dispatch]
pub(crate) trait PlatformImpl {
    #[allow(clippy::missing_errors_doc)]
    fn pump(&mut self) -> Result<(), crate::PumpError>;
    #[must_use]
    fn timestamp_granularity(&self) -> Option<std::time::Duration>;
    #[must_use]
    fn pads(&self) -> &[crate::pad::Pad];
    #[must_use]
    fn tools(&self) -> &[crate::tool::Tool];
    #[must_use]
    fn tablets(&self) -> &[crate::tablet::Tablet];
    #[must_use]
    fn make_summary(&self) -> crate::events::summary::Summary;
}
// temp :P
impl PlatformImpl for std::convert::Infallible {
    fn pump(&mut self) -> Result<(), crate::PumpError> {
        match *self {}
    }
    fn timestamp_granularity(&self) -> Option<std::time::Duration> {
        match *self {}
    }
    fn pads(&self) -> &[crate::pad::Pad] {
        match *self {}
    }
    fn tools(&self) -> &[crate::tool::Tool] {
        match *self {}
    }
    fn tablets(&self) -> &[crate::tablet::Tablet] {
        match *self {}
    }
    fn make_summary(&self) -> crate::events::summary::Summary {
        match *self {}
    }
}

/// Static dispatch between compiled backends.
/// Enum cause why not, (almost?) always has one variant and is thus compiles away to the inner type transparently.
/// Even empty enum is OK, since everything involving it becomes essentially `match ! {}` which is sound :D
#[enum_dispatch::enum_dispatch(PlatformImpl)]
pub(crate) enum PlatformManager {
    #[cfg(wl_tablet)]
    Wayland(wl::Manager),
    #[cfg(ink_rts)]
    Ink(std::convert::Infallible),
}