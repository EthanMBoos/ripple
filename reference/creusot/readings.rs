// Production reading-store and configured-input contracts. No simulator edits.
// Run: python3 reference/creusot/check.py --readings --mutations
//
// Checked: typed lookup; exact successful delivery/retention and freshness;
// clearing freshness preserves readings; latest/fresh select the first matching
// configured role, including its explicit perfect-measurement kind. Missing
// sensor data never falls back to truth. Error construction is abstracted.
//
// Boundaries: standard collection/iterator/Any specifications, Rust reference
// unsizing, and external physical-state types. Publication of owned dyn Any
// values is excluded (Creusot 0.14.0 resolve panic). Mission source selection,
// plugin callbacks, engine phase ordering, and all information flow through
// messages are not proved. The plugin-stack wiring review is separate evidence.
macro_rules! ensure {
    ($condition:expr, $($message:tt)*) => {
        if !$condition {
            return Err(reading_error());
        }
    };
}
#[trusted]
fn reading_error() -> anyhow::Error {
    anyhow::anyhow!("reading error")
}

type ErasedReading = dyn Any + Send + Sync;
extern_spec! {
    impl Clone for String {
        #[ensures(result@ == self@)]
        fn clone(&self) -> Self;
    }
    mod std { mod any { trait Any { fn type_id(&self) -> std::any::TypeId; } } }
    impl<K: Ord + DeepModel, V, A: std::alloc::Allocator + Clone> BTreeMap<K, V, A> {
        #[ensures(match result {
            None => self@.get(key.deep_model()) == None,
            Some(value) => self@.get(key.deep_model()) == Some(*value),
        })]
        fn get<'a, Q: ?Sized + Ord + DeepModel<DeepModelTy = K::DeepModelTy>>(&'a self, key: &Q) -> Option<&'a V>
        where K: std::borrow::Borrow<Q>;
        #[ensures(forall<key> (^self)@.get(key) == match other@.get(key) {
            Some(value) => Some(value), None => self@.get(key),
        })]
        #[ensures((^other)@ == creusot_std::logic::FMap::empty())]
        fn append(&mut self, other: &mut Self);
        #[ensures((^self)@ == creusot_std::logic::FMap::empty())]
        fn clear(&mut self);
    }
    impl<K: Ord + DeepModel, A: std::alloc::Allocator + Clone> BTreeSet<K, A> {
        #[ensures(result == self@.contains(value.deep_model()))]
        fn contains<Q: ?Sized + Ord + DeepModel<DeepModelTy = K::DeepModelTy>>(&self, value: &Q) -> bool
        where K: std::borrow::Borrow<Q>;
        #[ensures((^self)@ == creusot_std::logic::FSet::empty())]
        fn clear(&mut self);
        #[ensures((^self)@ == self@.insert(value.deep_model()))]
        fn insert(&mut self, value: K) -> bool;
    }
    impl ErasedReading {
        #[ensures(match result {
            Some(value) => any_cast::<T>(*self) == Some(*value),
            None => any_cast::<T>(*self) == None,
        })]
        fn downcast_ref<T: Any>(&self) -> Option<&T>;
    }
}
#[logic(opaque)]
fn any_cast<T: 'static>(_value: dyn Any + Send + Sync) -> Option<T> {
    dead
}

// Same restricted predicate assumptions as Creusot's filter model: callable,
// immutable captures, and a precise boolean result. Used by slice input lookup.
extern_spec! {
    mod core { mod iter {
        trait Iterator: creusot_std::std::iter::IteratorSpec {
            #[requires(forall<p: P, x: &Self::Item> inv(p) && inv(x) ==> p.precondition((x,)))]
            #[requires(forall<p: P, q: P> p.hist_inv(q) ==> p == q)]
            #[requires(forall<p: P, q: P, x: &Self::Item>
                !(p.postcondition_mut((x,), q, true) && p.postcondition_mut((x,), q, false)))]
            #[ensures(match result {
                Some(value) => exists<visited: Seq<Self::Item>> (*self).produces(visited.push_back(value), ^self) &&
                    predicate.postcondition_mut((&value,), predicate, true) &&
                    (forall<j> 0 <= j && j < visited.len() ==> predicate.postcondition_mut((&visited[j],), predicate, false)),
                None => exists<visited: Seq<Self::Item>, done: &mut Self>
                    (*self).produces(visited, *done) && done.completed() && ^self == ^done &&
                    (forall<j> 0 <= j && j < visited.len() ==> predicate.postcondition_mut((&visited[j],), predicate, false)),
            })]
            fn find<P>(&mut self, predicate: P) -> Option<Self::Item>
            where Self: Sized, P: FnMut(&Self::Item) -> bool;
        }
    } }
}

#[logic(inline)]
fn selected(sources: Seq<(String, Source)>, input: Seq<char>, index: Int) -> bool {
    pearlite! { 0 <= index && index < sources.len() && sources[index].0@ == input &&
    forall<j> 0 <= j && j < index ==> sources[j].0@ != input }
}

#[logic(inline)]
fn sensor_result<T: Send + Sync + 'static>(
    readings: Readings,
    sensor: Seq<char>,
    fresh_only: bool,
    result: Result<Option<Sample<'_, T>>>,
) -> bool {
    pearlite! {
        match readings.values@.get(sensor) {
            None => (match result { Ok(None) => true, _ => false }),
            Some(value) => match any_cast::<Reading<T>>(*value) {
                None => (match result { Err(_) => true, _ => false }),
                Some(reading) => if fresh_only && !readings.fresh@.contains(sensor) {
                    (match result { Ok(None) => true, _ => false })
                } else { match result {
                    Ok(Some(sample)) => sample.sampled_at_s == reading.sampled_at_s &&
                        *sample.value == reading.value && sample.fresh == readings.fresh@.contains(sensor),
                    _ => false,
                } },
            },
        }
    }
}

#[logic(inline)]
fn source_result<T: Send + Sync + 'static>(
    inputs: Inputs<'_>,
    source: Source,
    fresh_only: bool,
    result: Result<Option<Sample<'_, T>>>,
) -> bool {
    pearlite! {
        match source {
            Source::Sensor(sensor) => sensor_result(*inputs.readings, sensor@, fresh_only, result),
            Source::Perfect(kind) => {
                let converted = match kind {
                    ReadingKind::State => cast_between::<T, KinematicState>(*inputs.perfect_state),
                    _ => cast_between::<T, ContactList>(*inputs.perfect_contacts),
                };
                match converted {
                    Some(value) => match result {
                        Ok(Some(sample)) => *sample.value == value &&
                            sample.sampled_at_s == inputs.now_s && sample.fresh,
                        _ => false,
                    },
                    None => (match result { Err(_) => true, _ => false }),
                }
            },
        }
    }
}

#[logic(inline)]
fn input_result<T: Send + Sync + 'static>(
    inputs: Inputs<'_>,
    input: Seq<char>,
    fresh_only: bool,
    result: Result<Option<Sample<'_, T>>>,
) -> bool {
    pearlite! {
        (exists<i> selected(inputs.sources@, input, i) &&
            source_result(inputs, inputs.sources@[i].1, fresh_only, result)) ||
        ((forall<i> 0 <= i && i < inputs.sources@.len() ==> inputs.sources@[i].0@ != input) &&
            (match result { Err(_) => true, _ => false }))
    }
}

// The reference coercion and Any downcast are modeled together because Creusot
// 0.14.0 represents repeated dyn-to-dyn coercions as unrelated values.
#[logic(opaque)]
fn cast_between<T: 'static, U: Any>(_value: U) -> Option<T> {
    dead
}
#[trusted]
#[ensures(match result {
    Some(value) => cast_between::<T, U>(*source) == Some(*value),
    None => cast_between::<T, U>(*source) == None,
})]
fn cast_ref<T: Any, U: Any>(source: &U) -> Option<&T> {
    (source as &dyn Any).downcast_ref::<T>()
}

#[logic]
#[requires(creusot_std::std::slice::SliceExt::to_ref_seq(sources) == prefix.push_back(found).concat(rest))]
#[requires(found.0@ == input)]
#[requires(forall<j> 0 <= j && j < prefix.len() ==> prefix[j].0@ != input)]
#[ensures(selected(sources@, input, prefix.len()) && sources@[prefix.len()].1 == found.1)]
fn first_input(
    sources: &[(String, Source)],
    prefix: Seq<&(String, Source)>,
    found: &(String, Source),
    rest: Seq<&(String, Source)>,
    input: Seq<char>,
) {
    proof_assert!(sources@[prefix.len()] == *found);
    proof_assert!(forall<j> 0 <= j && j < prefix.len() ==> sources@[j] == *prefix[j]);
}
