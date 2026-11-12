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

// Read-only map iteration is modeled as a sequence of entries from the map.
// We need membership, not key order, for recipient soundness.
#[cfg(creusot)]
mod btree_iteration {
    use crate::{prelude::*, std::iter::IteratorSpec};
    use std::{
        alloc::Allocator,
        collections::{BTreeMap, btree_map::Iter},
    };
    impl<'a, K: DeepModel, V> View for Iter<'a, K, V> {
        type ViewTy = Seq<(&'a K, &'a V)>;
        #[logic(opaque)]
        fn view(self) -> Self::ViewTy {
            dead
        }
    }
    #[logic(opaque)]
    pub fn source<K: DeepModel, V>(_iter: Iter<'_, K, V>) -> crate::logic::FMap<K::DeepModelTy, V> {
        dead
    }
    impl<'a, K: DeepModel, V> IteratorSpec for Iter<'a, K, V> {
        #[logic(open, prophetic)]
        fn produces(self, visited: Seq<Self::Item>, other: Self) -> bool {
            pearlite! {
                self@ == visited.concat(other@) && source(self) == source(other) &&
                forall<i> 0 <= i && i < visited.len() ==>
                    source(self).get(visited[i].0.deep_model()) == Some(*visited[i].1)
            }
        }
        #[logic(open, prophetic)]
        fn completed(&mut self) -> bool {
            pearlite! { resolve(self) && self@.len() == 0 }
        }
        #[logic(law)]
        #[ensures(self.produces(Seq::empty(), self))]
        fn produces_refl(self) {}
        #[logic(law)]
        #[requires(a.produces(ab, b))]
        #[requires(b.produces(bc, c))]
        #[ensures(a.produces(ab.concat(bc), c))]
        fn produces_trans(a: Self, ab: Seq<Self::Item>, b: Self, bc: Seq<Self::Item>, c: Self) {}
    }
    extern_spec! {
        impl<'a, K: DeepModel, V, A: Allocator + Clone> IntoIterator for &'a BTreeMap<K, V, A> {
            #[ensures(source(result) == self@)]
            #[ensures(result@.len() == self@.len())]
            #[ensures(forall<i> 0 <= i && i < result@.len() ==>
                self@.get(result@[i].0.deep_model()) == Some(*result@[i].1))]
            fn into_iter(self) -> Iter<'a, K, V>;
        }
    }
}
