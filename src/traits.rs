use std::future::Future;
use crate::signal_map::MutableSignalMap;
use crate::store::{SpawnedFutKey, StoreArcMutexGuard};

pub trait SSS: Send + Sync + 'static {}

impl<T: Send + Sync + 'static> SSS for T {}

pub trait Get<T: Copy> {
    fn get(&self) -> T;
}

pub trait GetCloned<T: Clone> {
    fn get_cloned(&self) -> T;
}

pub trait HasSignal<T: Copy> {
    type Return;//: Signal + Send + Sync + 'static;

    fn signal(&self) -> Self::Return;
}

pub trait HasSignalCloned<T: Clone> {
    type Return;//: Signal + Send + Sync + 'static;

    fn signal_cloned(&self) -> Self::Return;
}

pub trait HasSignalMap<K, V>
    where
        K: Copy + Ord,
        V: Copy
{
    fn signal_map(&self) -> MutableSignalMap<K, V>;
}

pub trait HasSignalMapCloned<K, V>
    where
        K: Clone + Ord,
        V: Clone
{
    fn signal_map_cloned(&self) -> MutableSignalMap<K, V>;
}

pub trait Provider {
    fn fut_key(&self) -> Option<SpawnedFutKey>;
}

pub trait ObserveMap<T, O, K, V, F, U>
    where
        T: HasSignalMap<K, V> + Provider,
        O: IsObservable,
        <O as IsObservable>::Inner: Clone,
        K: Copy + Ord,
        V: Copy,
        F: Fn(MutableSignalMap<K, V>, <O as IsObservable>::Inner) -> U,
        U: Future<Output = ()>  + Send + 'static
{
    fn observe_map(input: T, store_ref: StoreArcMutexGuard, f: F) -> O;
}

pub trait ObserveMapCloned<T, O, K, V, F, U>
    where
        T: HasSignalMapCloned<K, V> + Provider,
        O: IsObservable,
        <O as IsObservable>::Inner: Clone,
        K: Clone + Ord,
        V: Clone,
        F: Fn(MutableSignalMap<K, V>, <O as IsObservable>::Inner) -> U,
        U: Future<Output = ()>  + Send + 'static
{
    fn observe_map_cloned(input: T, store_ref: StoreArcMutexGuard, f: F) -> O;
}

pub trait IsObservable {
    type Inner;

    fn new_inner() -> Self::Inner;

    fn new(inner: Self::Inner, fut_key: SpawnedFutKey) -> Self;
}