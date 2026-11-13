#![cfg_attr(creusot, feature(allocator_api))]
#![allow(dead_code)]
#[cfg(feature = "routing")]
include!("routing.rs");
#[cfg(feature = "readings")]
include!("readings.rs");
include!(concat!(env!("OUT_DIR"), "/messages.rs"));

#[cfg(not(feature = "readings"))]
#[logic(opaque)]
fn type_key<T: ?Sized + 'static>() -> TypeId {
    dead
}

#[cfg(not(feature = "readings"))]
extern_spec! {
    impl TypeId {
        #[ensures(result == type_key::<T>())]
        fn of<T: ?Sized + 'static>() -> TypeId;
    }
    impl<K: Ord + DeepModel, V, A: std::alloc::Allocator + Clone> BTreeMap<K, V, A> {
        #[ensures(match result {
            None => self@.get(key.deep_model()) == None,
            Some(v) => self@.get(key.deep_model()) == Some(*v),
        })]
        fn get<'a, Q: ?Sized + Ord + DeepModel<DeepModelTy = K::DeepModelTy>>(&'a self, key: &Q) -> Option<&'a V>
        where K: std::borrow::Borrow<Q>;

        #[ensures(result == (*self)@.get(key.deep_model()))]
        #[ensures((^self)@ == (*self)@.remove(key.deep_model()))]
        fn remove<Q: ?Sized + Ord + DeepModel<DeepModelTy = K::DeepModelTy>>(&mut self, key: &Q) -> Option<V>
        where K: std::borrow::Borrow<Q>;
    }
}

#[cfg(not(feature = "readings"))]
#[logic(opaque)]
fn cast<T: Any + Send + Sync, A: std::alloc::Allocator, P>(value: P) -> Option<Arc<T, A>> {
    dead
}
#[cfg(not(feature = "readings"))]
extern_spec! {
    impl<A: std::alloc::Allocator> Arc<dyn Any + Send + Sync, A> {
        #[ensures(match result {
            Ok(value) => cast::<T, A, _>(self) == Some(value),
            Err(_) => cast::<T, A, _>(self) == None,
        })]
        fn downcast<T: Any + Send + Sync>(self) -> Result<Arc<T, A>, Self>;
    }
}

#[cfg(not(feature = "readings"))]
#[logic]
fn subscribed<T: 'static>(messages: Messages, network: Seq<char>, topic: Seq<char>) -> bool {
    pearlite! {
       messages.subscriptions@.get((network, topic)) == Some(type_key::<T>())
    }
}
#[cfg(not(feature = "readings"))]
#[logic]
fn queue(messages: Messages, network: Seq<char>, topic: Seq<char>) -> Seq<Envelope> {
    pearlite! {
       match messages.inbox@.get((network, topic)) {
           None => Seq::empty(),
           Some(q) => q@,
       }
    }
}
#[cfg(not(feature = "readings"))]
#[logic]
fn converted<T: Any + Send + Sync>(before: Envelope, after: Message<T>) -> bool {
    pearlite! {
       before.sender == after.sender && before.sent_at_s == after.sent_at_s &&
       before.delivered_at_s == after.delivered_at_s &&
       cast::<T, std::alloc::Global, _>(before.value) == Some(after.value)
    }
}
#[cfg(not(feature = "readings"))]
#[logic]
fn conversion_prefix<T: Any + Send + Sync>(before: Seq<Envelope>, after: Seq<Message<T>>) -> bool {
    pearlite! {
       before.len() == after.len() &&
       forall<i> 0 <= i && i < before.len() ==> converted(before[i], after[i])
    }
}
