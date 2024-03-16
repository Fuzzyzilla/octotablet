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
    Wayland(wl::ID),
    #[cfg(ink_rts)]
    Ink(ink::ID),
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
/// and no way to share IDs between managers of different backends. Thus, the only way this can fail is e.g. the wayland
/// backend creating an Ink ID.
///
/// In most (all?) compilation environments, these are infallible and compile down to nothing, hence the inline (profile me :3).
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
    pub(crate) fn unwrap_wl(&self) -> &wl::ID {
        #[allow(unreachable_patterns)]
        #[allow(clippy::match_wildcard_for_single_variants)]
        match self {
            Self::Wayland(id) => id,
            _ => Self::unwrap_failure(),
        }
    }
    #[cfg(ink_rts)]
    #[inline]
    pub(crate) fn unwrap_ink(&self) -> &ink::ID {
        #[allow(unreachable_patterns)]
        #[allow(clippy::match_wildcard_for_single_variants)]
        match self {
            Self::Ink(id) => id,
            _ => Self::unwrap_failure(),
        }
    }
}
#[cfg(wl_tablet)]
impl From<wl::ID> for InternalID {
    fn from(value: wl::ID) -> Self {
        Self::Wayland(value)
    }
}
#[cfg(ink_rts)]
impl From<ink::ID> for InternalID {
    fn from(value: ink::ID) -> Self {
        Self::Ink(value)
    }
}
/// Holds any one of the internal platform IDs.
/// Since these are always sealed away as an implementation detail, we can always
/// assume they're the right type since they can never be moved between `Manager`s.
// (because of this it could actually be a union. hmmmm...)
#[derive(Copy, Clone, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum ButtonID {
    #[cfg(wl_tablet)]
    Wayland(wl::ButtonID),
    #[cfg(ink_rts)]
    Ink(ink::ButtonID),
}
impl std::fmt::Debug for ButtonID {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        use std::hash::{Hash, Hasher};
        // We display as a hash since it's an opaque object, but we still want visual distinction between
        // differing IDs.
        // We *really* don't care what the results are here, as long as it's consistent during a single run.
        // Rather than pull in a dep, just use a random hasher from std!
        let mut h = std::collections::hash_map::DefaultHasher::new();
        self.hash(&mut h);
        f.debug_tuple("ButtonID").field(&h.finish()).finish()
    }
}

/// Unwrappers. Impls are free to assume their IDs are always the right type, as there are no accessors
/// and no way to share IDs between managers of different backends. Thus, the only way this can fail is e.g. the wayland
/// backend creating an Ink ID.
///
/// In most (all?) compilation environments, these are infallible and compile down to nothing, hence the inline (profile me :3).
impl ButtonID {
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
    pub(crate) fn unwrap_wl(&self) -> &wl::ButtonID {
        #[allow(unreachable_patterns)]
        #[allow(clippy::match_wildcard_for_single_variants)]
        match self {
            Self::Wayland(id) => id,
            _ => Self::unwrap_failure(),
        }
    }
    #[cfg(ink_rts)]
    #[inline]
    pub(crate) fn unwrap_ink(&self) -> &ink::ButtonID {
        #[allow(unreachable_patterns)]
        #[allow(clippy::match_wildcard_for_single_variants)]
        match self {
            Self::Ink(id) => id,
            _ => Self::unwrap_failure(),
        }
    }
}
#[cfg(wl_tablet)]
impl From<wl::ButtonID> for ButtonID {
    fn from(value: wl::ButtonID) -> Self {
        Self::Wayland(value)
    }
}
#[cfg(ink_rts)]
impl From<ink::ButtonID> for ButtonID {
    fn from(value: ink::ButtonID) -> Self {
        Self::Ink(value)
    }
}

pub(crate) enum RawEventsIter<'a> {
    #[cfg(wl_tablet)]
    Wayland(std::slice::Iter<'a, crate::events::raw::Event<wl::ID>>),
    #[cfg(ink_rts)]
    Ink(std::slice::Iter<'a, crate::events::raw::Event<ink::ID>>),
}
impl Iterator for RawEventsIter<'_> {
    type Item = crate::events::raw::Event<InternalID>;
    fn next(&mut self) -> Option<Self::Item> {
        // This is still a branch per iten, sadness! Not sure a cheaper way to go about it.
        match self {
            #[cfg(wl_tablet)]
            Self::Wayland(wl) => wl.next().cloned().map(crate::events::raw::Event::id_into),
            #[cfg(ink_rts)]
            Self::Ink(ink) => ink.next().cloned().map(crate::events::raw::Event::id_into),
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
    fn raw_events(&self) -> RawEventsIter<'_>;
}

/// Static dispatch between compiled backends.
/// Enum cause why not, (almost?) always has one variant and is thus compiles away to the inner type transparently.
/// Even empty enum is OK, since everything involving it becomes essentially `match ! {}` which is sound :D
#[enum_dispatch::enum_dispatch(PlatformImpl)]
pub(crate) enum PlatformManager {
    #[cfg(wl_tablet)]
    Wayland(wl::Manager),
    #[cfg(ink_rts)]
    Ink(ink::Manager),
}
