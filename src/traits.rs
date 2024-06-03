use std::future::Future;
use crate::observable::Observable;
use crate::signal::Signal;

use crate::signal_map::MutableSignalMap;
use crate::store::{Manager, SpawnedFutureKey, StoreAccess, StoreArcMutexGuard, StoreHandle};

pub trait SSS: Send + Sync + 'static {}

impl<T: Send + Sync + 'static> SSS for T {}

pub trait Get<T: Copy> {
    fn get(&self) -> T;
}

pub trait GetCloned<T: Clone> {
    fn get_cloned(&self) -> T;
}

pub trait HasSignal<T: Copy> {
    type Return;

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

pub(crate) trait HasStoreHandle {
    fn store_handle(&self) -> &StoreHandle;
}

pub trait Provider {
    fn fut_key(&self) -> Option<SpawnedFutureKey>;
}

// pub trait Observe {
//     fn observe<U, F, I>(&self, f: F) -> Observable<U> where F: Fn(I) -> U + Send + Sync + 'static;
// }

//pub trait OutputMap<K, V>

pub trait ObserveMap<T, O, K, V, F, U>
    where
        Self: HasSignalMap<K, V> + Provider + HasStoreHandle,
        O: IsObservable,
        <O as IsObservable>::Inner: Clone,
        K: Copy + Ord,
        V: Copy,
        F: Fn(MutableSignalMap<K, V>, <O as IsObservable>::Inner) -> U,
        U: Future<Output = ()>  + Send + 'static
{
    fn observe_map(&self, f: F) -> O;
}

pub trait ObserveMapCloned<T, O, K, V, F, U>
    where
        T: HasSignalMapCloned<K, V> + Provider + HasStoreHandle,
        O: IsObservable,
        <O as IsObservable>::Inner: Clone,
        K: Clone + Ord,
        V: Clone,
        F: Fn(MutableSignalMap<K, V>, <O as IsObservable>::Inner) -> U,
        U: Future<Output = ()>  + Send + 'static
{
    fn observe_map_cloned(&self, f: F) -> O;
}

pub trait IsObservable {
    type Inner;

    fn new_inner(store_handle: StoreHandle) -> Self::Inner;

    fn new(store_handle: StoreHandle, inner: Self::Inner, fut_key: SpawnedFutureKey) -> Self;
}

pub trait HasSpawnedFutureKey {
    fn spawned_future_key(&self) -> SpawnedFutureKey;
}