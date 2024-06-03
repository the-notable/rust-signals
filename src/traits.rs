use std::future::Future;
use crate::signal::{Signal, SignalExt};

use crate::signal_map::MutableSignalMap;
use crate::store::{SpawnedFutureKey, StoreAccess, StoreHandle};

pub trait SSS: Send + Sync + 'static {}

impl<T: Send + Sync + 'static> SSS for T {}

pub trait Get<T: Clone> {
    fn get(&self) -> T;
}

// pub trait GetCloned<T: Clone> {
//     fn get_cloned(&self) -> T;
// }

pub trait HasSignal<T: Clone> {
    type Return;

    fn signal(&self) -> Self::Return;
}

// pub trait HasSignalCloned<T: Clone> {
//     type Return;//: Signal + Send + Sync + 'static;
// 
//     fn signal_cloned(&self) -> Self::Return;
// }

pub trait HasSignalMap<K, V>
    where
        K: Clone + Ord,
        V: Clone
{
    fn signal_map(&self) -> MutableSignalMap<K, V>;
}

// pub trait HasSignalMapCloned<K, V>
//     where
//         K: Clone + Ord,
//         V: Clone
// {
//     fn signal_map_cloned(&self) -> MutableSignalMap<K, V>;
// }

pub trait HasStoreHandle {
    fn store_handle(&self) -> &StoreHandle;
}

pub trait Provider: HasStoreHandle {
    type YieldedValue;
    
    fn fut_key(&self) -> Option<SpawnedFutureKey>;

    fn register_effectt<F>(&self, f: F) -> Result<SpawnedFutureKey, &'static str>
        where
            F: Fn(Self::YieldedValue) + Send + 'static;
    
    fn register_effect<T, F>(&self, f: F) -> Result<SpawnedFutureKey, &'static str>
        where 
            T: Copy,
            Self: HasSignal<T>,
            <Self as HasSignal<T>>::Return: Signal + Send + 'static,
            F: Fn(<<Self as HasSignal<T>>::Return as Signal>::Item) + Send + 'static
    {
        //let source_cloned = source.clone();
        let fut = self.signal().for_each(move |v| {
            f(v);
            async {}
        });

        let mut lock = self.store_handle().clone();
        let key = lock.spawn_fut(self.fut_key(), fut);
        Ok(key)
    }
}

// pub trait Observe {
//     fn observe<U, F, I>(&self, f: F) -> Observable<U> where F: Fn(I) -> U + Send + Sync + 'static;
// }

//pub trait OutputMap<K, V>

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

// pub trait ObserveMapCloned<T, O, K, V, F, U>
//     where
//         T: HasSignalMapCloned<K, V> + Provider,
//         O: IsObservable,
//         <O as IsObservable>::Inner: Clone,
//         K: Clone + Ord,
//         V: Clone,
//         F: Fn(MutableSignalMap<K, V>, <O as IsObservable>::Inner) -> U,
//         U: Future<Output = ()>  + Send + 'static
// {
//     fn observe_map_cloned(&self, f: F) -> O;
// }

pub(crate) trait IsObservable {
    type Inner;

    fn new_inner(store_handle: StoreHandle) -> Self::Inner;

    fn new(store_handle: StoreHandle, inner: Self::Inner, fut_key: SpawnedFutureKey) -> Self;
}

pub trait HasSpawnedFutureKey {
    fn spawned_future_key(&self) -> SpawnedFutureKey;
}