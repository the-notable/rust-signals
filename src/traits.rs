use std::future::Future;

use crate::signal_map::MutableSignalMap;
use crate::store::{SpawnedFutureKey, StoreHandle};

pub trait SSS: Send + Sync + 'static {}

impl<T: Send + Sync + 'static> SSS for T {}

pub trait Get<T: Clone> {
    fn get(&self) -> T;
}

pub trait HasSignal<T: Clone> {
    type Return;

    fn signal(&self) -> Self::Return;
}

pub trait HasSignalMap<K, V>
    where
        K: Clone + Ord,
        V: Clone
{
    fn signal_map(&self) -> MutableSignalMap<K, V>;
}

pub trait HasStoreHandle {
    fn store_handle(&self) -> &StoreHandle;
}

pub trait Provider: HasStoreHandle {
    type YieldedValue;
    
    fn fut_key(&self) -> Option<SpawnedFutureKey>;

    fn register_effect<F>(&self, f: F) -> Result<SpawnedFutureKey, &'static str>
        where
            F: Fn(Self::YieldedValue) + Send + 'static;
}

pub trait ObserveMap<T, O, K, V, F, U>
    where
        Self: HasSignalMap<K, V> + Provider,
        O: IsObservable,
        <O as IsObservable>::Inner: Clone,
        K: Clone + Ord,
        V: Clone,
        F: Fn(MutableSignalMap<K, V>, <O as IsObservable>::Inner) -> U,
        U: Future<Output = ()>  + Send + 'static
{
    fn observe_map(&self, f: F) -> O;
}

pub(crate) trait IsObservable {
    type Inner;

    fn new_inner(store_handle: StoreHandle) -> Self::Inner;

    fn new(store_handle: StoreHandle, inner: Self::Inner, fut_key: SpawnedFutureKey) -> Self;
}

pub trait HasSpawnedFutureKey {
    fn spawned_future_key(&self) -> SpawnedFutureKey;
}