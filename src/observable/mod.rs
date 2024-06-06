use crate::signal::{Mutable, MutableSignal};
use crate::signal::SignalExt;
use crate::store::{SpawnedFutureKey, StoreAccess, StoreHandle};
use crate::traits::{HasSignal, HasSpawnedFutureKey, HasStoreHandle, Provider, SSS};

#[derive(Debug, Clone)]
pub struct Observable<T> {
    pub(crate) store_handle: StoreHandle,
    pub(crate) mutable: Mutable<T>,
    pub(crate) fut_key: SpawnedFutureKey
}

impl<T: Clone + SSS> Provider for Observable<T> {
    type YieldedValue = T;
    
    fn fut_key(&self) -> Option<SpawnedFutureKey> {
        Some(self.spawned_future_key())
    }

    fn register_effect<F>(&self, f: F) -> Result<SpawnedFutureKey, &'static str> 
        where 
            F: Fn(Self::YieldedValue) + Send + 'static 
    {
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
        self.fut_key
    }
}

impl<T: Clone> Observable<T> {
    pub fn get(&self) -> T {
        self.mutable.get()
    }
}

// impl<A> Observe<A> for Observable<A>
//     where
//         <Self as HasSignal<A>>::Return: Signal + Send + Sync + 'static,
//         A: Clone + Send + Sync + 'static
// {}

// impl<A> ObserveCloned<A> for Observable<A>
//     where
//         <Self as HasSignalCloned<A>>::Return: Signal + Send + Sync + 'static,
//         A: Clone + Send + Sync + 'static
// {}

impl<A> HasStoreHandle for Observable<A> {
    fn store_handle(&self) -> &StoreHandle {
        &self.store_handle
    }
}

impl<T: Clone> Observable<T> {
    pub fn get_cloned(&self) -> T {
        self.mutable.get_cloned()
    }
}

impl<A: Clone> HasSignal<A> for Observable<A> {
    type Return = MutableSignal<A>;

    fn signal(&self) -> Self::Return {
        self.mutable.signal()
    }
}

// pub trait Observe<A>: HasSignal<A> + Provider
//     where
//         <Self as HasSignal<A>>::Return: Signal + Send + Sync + 'static + Iterator,
//         A: Clone + Send + Sync + 'static
// {
//     fn observe<U, F>(&self, f: F) 
//         -> Observable<U>
//         where
//             U: Default + Send + Sync + 'static,
//             F: Fn(<<Self as HasSignal<A>>::Return as Signal>::Item) -> U + Send + Sync + 'static
//     {
//         let mut store_handle = self.store_handle().clone();
//         let out = store_handle.new_mutable(U::default());
//         let out_mutable_clone = out.clone();
//         let fut = self.signal().for_each(move |v| {
//             out_mutable_clone.set(f(v));
//             async {}
//         });
//         
//         let fut_key = store_handle.spawn_fut(None, fut);
//         Observable {
//             store_handle,
//             mutable: out,
//             fut_key
//         }
//     }
// }

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};
    use std::thread::sleep;
    use std::time::Duration;

    use crate::signal::ObserveSignal;
    use crate::signal::SignalExt;
    use crate::store::{Manager, RxStore};
    use crate::traits::{HasSignal, Provider};

    #[test]
    fn it_has_future_key() {
        let store = RxStore::new();
        let mutable = store.new_mutable(1);
        let observable = mutable
            .signal()
            .map(|v| v)
            .observe();
        assert!(observable.fut_key().is_some())
    }
    
    #[test]
    fn it_registers_effect() {
        let store = RxStore::new();
        let mutable = store.new_mutable(1);
        let observable = mutable
            .signal()
            .map(|v| v)
            .observe();
        let holder = Arc::new(Mutex::new(0));
        let holder_clone = holder.clone();
        observable.register_effect(move |v| {
            let mut lock = holder_clone.lock().unwrap();
            *lock = v + 1;
        }).unwrap();
        mutable.set(2);
        sleep(Duration::from_millis(500));
        let v = {
            let lock = holder.lock().unwrap();
            *lock
        };
        assert_eq!(3, v)
    }
}