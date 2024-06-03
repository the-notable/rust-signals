use crate::signal::{Mutable, MutableSignal, MutableSignalCloned, Signal};
use crate::store::{Manager, SpawnedFutureKey, StoreAccess, StoreHandle};
use crate::signal::SignalExt;
use crate::traits::{HasSignal, HasSignalCloned, HasSpawnedFutureKey, Provider};

#[derive(Debug, Clone)]
pub struct Observable<T> {
    store_handle: StoreHandle,
    pub(crate) mutable: Mutable<T>,
    pub(crate) fut_key: SpawnedFutureKey
}

impl<T> Provider for Observable<T> {
    fn fut_key(&self) -> Option<SpawnedFutureKey> {
        Some(self.spawned_future_key())
    }

    fn store_handle(&self) -> &StoreHandle {
        &self.store_handle
    }

    fn register_effectt<R, F, U>(&self, f: F) -> Result<SpawnedFutureKey, &'static str> where R: Copy, F: Fn(U) + Send + 'static {
        let fut = self.signal().for_each(move |v| {
            f(v);
            async {}
        });

        let mut lock = self.store_handle().clone();
        let key = lock.spawn_fut(self.fut_key(), fut);
        Ok(key)
    }
}

impl<T> HasSpawnedFutureKey for Observable<T> {
    fn spawned_future_key(&self) -> SpawnedFutureKey {
        self.fut_key.clone()
    }
}

impl<T: Copy> Observable<T> {
    pub fn get(&self) -> T {
        self.mutable.get()
    }
}

impl<A> Observe<A> for Observable<A>
    where
        <Self as HasSignal<A>>::Return: Signal + Send + Sync + 'static,
        A: Copy + Send + Sync + 'static
{}

impl<A> ObserveCloned<A> for Observable<A>
    where
        <Self as HasSignalCloned<A>>::Return: Signal + Send + Sync + 'static,
        A: Clone + Send + Sync + 'static
{}

impl<T: Clone> Observable<T> {
    pub fn get_cloned(&self) -> T {
        self.mutable.get_cloned()
    }
}

impl<A: Copy> HasSignal<A> for Observable<A> {
    type Return = MutableSignal<A>;

    fn signal(&self) -> Self::Return {
        self.mutable.signal()
    }
}

impl<A: Clone> HasSignalCloned<A> for Observable<A> {
    type Return = MutableSignalCloned<A>;

    fn signal_cloned(&self) -> Self::Return {
        self.mutable.signal_cloned()
    }
}

pub trait Observe<A>: HasSignal<A>
    where
        <Self as HasSignal<A>>::Return: Signal + Send + Sync + 'static,
        A: Copy + Send + Sync + 'static
{
    fn observe<U, F>(&self, f: F) 
        -> Observable<U>
        where
            U: Default + Send + Sync + 'static,
            F: Fn(<<Self as HasSignal<A>>::Return as Signal>::Item) -> U + Send + Sync + 'static
    {
        let mut store_handle = self.store_handle().clone();
        let out = store_handle.new_mutable(U::default());
        let out_mutable_clone = out.clone();
        let fut = self.signal().for_each(move |v| {
            out_mutable_clone.set(f(v));
            async {}
        });

        //let mut lock = store_handle.get_store();
        let fut_key = store_handle.spawn_fut(None, fut);
        Observable {
            store_handle,
            mutable: out,
            fut_key
        }
    }
}

pub trait ObserveCloned<A>: HasSignalCloned<A>
    where
        <Self as HasSignalCloned<A>>::Return: Signal + Send + Sync + 'static,
        A: Clone + Send + Sync + 'static
{
    fn observe_cloned<U, F>(&self, f: F)
                            -> Observable<U>
        where
            U: Default + Send + Sync + 'static,
            F: Fn(<<Self as HasSignalCloned<A>>::Return as Signal>::Item) -> U + Send + Sync + 'static
    {
        let mut store_handle = self.store_handle().clone();
        let out = store_handle.new_mutable(U::default());
        let out_mutable_clone = out.clone();
        let fut = self.signal_cloned().for_each(move |v| {
            out_mutable_clone.set(f(v));
            async {  }
        });

        //let mut lock = store_handle.get_store();
        let fut_key = store_handle.spawn_fut(None, fut);
        Observable {
            store_handle,
            mutable: out,
            fut_key
        }
    }
}