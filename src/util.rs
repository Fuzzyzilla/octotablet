#[derive(thiserror::Error, Debug)]
pub enum NicheF32Error {
    /// Attempted to make a non-NaN value our of NaN.
    #[error("provided value was NaN")]
    NaN,
}

/// An Option type where NaN is the niche.
// Todo: manual Ord that makes it not partial.
#[derive(Copy, Clone, PartialOrd)]
pub struct NicheF32(f32);
impl NicheF32 {
    pub const NONE: NicheF32 = NicheF32(f32::NAN);
    /// Wrap a float in this niche, `NaN` coercing to `None`.
    // Not pub cause it might be a footgun lol
    #[must_use]
    const fn wrap(value: f32) -> Self {
        Self(value)
    }
    /// Wrap a non-`NaN` value. Fails with `None` if the value was `NaN`.
    #[must_use]
    pub fn new_some(value: f32) -> Option<Self> {
        (!value.is_nan()).then_some(Self::wrap(value))
    }
    /// Get a `None` niche.
    #[must_use]
    pub const fn new_none() -> Self {
        Self::NONE
    }
    /// Get the optional value within. If `Some`, guaranteed to not be `NaN`.
    #[must_use]
    pub fn get(self) -> Option<f32> {
        (!self.0.is_nan()).then_some(self.0)
    }
    /// Create from an [`Option`]. `Some` and `None` variants will correspond exactly with return value of `self.get()`.
    /// # Safety
    /// The value must not be `Some(NaN)`.
    #[must_use]
    pub unsafe fn from_option_unchecked(value: Option<f32>) -> Self {
        unsafe { value.try_into().unwrap_unchecked() }
    }
}
impl TryFrom<Option<f32>> for NicheF32 {
    type Error = NicheF32Error;
    fn try_from(value: Option<f32>) -> Result<Self, Self::Error> {
        if value.is_some_and(f32::is_nan) {
            Err(NicheF32Error::NaN)
        } else {
            // Not Some(NAN), so we can convert.
            Ok(NicheF32(value.unwrap_or(f32::NAN)))
        }
    }
}
impl Default for NicheF32 {
    fn default() -> Self {
        // Not a zero-pattern which is typical of most primitives,
        // but more reasonable than Some(0.0) being the default.
        Self::new_none()
    }
}
impl std::fmt::Debug for NicheF32 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self.get())
    }
}
impl PartialEq for NicheF32 {
    fn eq(&self, other: &Self) -> bool {
        // All NaNs are filtered to None (and considered to be equal here)
        // The remaining f32 comp is Full.
        self.get() == other.get()
    }
}
// One side being non-nan makes it no longer partial!
impl Eq for NicheF32 {}
impl PartialEq<f32> for NicheF32 {
    fn eq(&self, other: &f32) -> bool {
        if let Some(value) = self.get() {
            value == *other
        } else {
            false
        }
    }
}
impl PartialEq<NicheF32> for f32 {
    fn eq(&self, other: &NicheF32) -> bool {
        other == self
    }
}
