// Appended only to the proof's local copy of creusot-std 0.14.0.
// TypeId equality uses identity; map contents are an abstract finite map.
impl crate::model::DeepModel for core::any::TypeId {
    type DeepModelTy = Self;
    #[crate::macros::logic(open, inline)]
    fn deep_model(self) -> Self {
        self
    }
}

#[cfg(creusot)]
impl<K: crate::model::DeepModel, V, A: ::std::alloc::Allocator + Clone> crate::model::View
    for ::std::collections::BTreeMap<K, V, A>
{
    type ViewTy = crate::logic::FMap<K::DeepModelTy, V>;
    #[crate::macros::logic(opaque)]
    fn view(self) -> Self::ViewTy {
        dead
    }
}
