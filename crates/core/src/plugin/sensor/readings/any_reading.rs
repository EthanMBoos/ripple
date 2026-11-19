//! A reading of any type, boxed so one map can hold every sensor's.
use super::Reading;
use std::any::Any;

/// A `Reading<T>` of any `T`, as its sensor published it; `Readings::get`
/// recovers it by type.
pub struct AnyReading(Box<dyn Any + Send + Sync>);
impl AnyReading {
    pub(super) fn new<T: Send + Sync + 'static>(reading: Reading<T>) -> Self {
        Self(Box::new(reading))
    }
    pub(super) fn downcast_ref<T: 'static>(&self) -> Option<&Reading<T>> {
        self.0.downcast_ref()
    }
}
