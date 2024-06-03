use std::collections::BTreeMap;
use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use parking_lot::{Mutex, RawMutex};
use parking_lot::lock_api::ArcMutexGuard;
use slotmap::{new_key_type, SlotMap};
use state::TypeMap;
use tokio::runtime::{Builder, Handle, Runtime};
use tokio::select;
use tokio_util::sync::CancellationToken;

use crate::signal::{Mutable};
use crate::signal_map::MutableBTreeMap;
use crate::signal_vec::MutableVec;
use crate::traits::{HasStoreHandle};

new_key_type! {
    pub struct SpawnedFutureKey;
}

pub(crate) type StorePtr = Arc<Mutex<StoreInner>>;
pub(crate) type StoreArcMutexGuard = ArcMutexGuard<RawMutex, StoreInner>;

pub trait Manager: HasStoreHandle {
    fn set<T: Send + Sync + 'static>(&self, v: T) -> bool {
        let lock = self.store_handle().get_store();
        lock.set(Arc::new(v))
    }

    fn get<T: Send + Sync + 'static>(&self) -> Arc<T> {
        let lock = self.store_handle().get_store();
        lock.get::<T>().clone()
    }
    
    fn new_mutable<T: 'static>(&self, v: T) -> Mutable<T> {
        Mutable::new(v, self.store_handle().clone())
    }
    
    fn new_mutable_vec<T>(&self) -> MutableVec<T> {
        MutableVec::new(self.store_handle().clone())
    }

    fn new_mutable_vec_w_values<T>(&self, v: Vec<T>) -> MutableVec<T> {
        MutableVec::new_with_values(v, self.store_handle().clone())
    }

    fn new_mutable_vec_w_capacity<T>(&self, capacity: usize) -> MutableVec<T> {
        MutableVec::with_capacity(capacity, self.store_handle().clone())
    }
    
    fn new_mutable_btree_map<K: Ord, V>(&self) -> MutableBTreeMap<K, V> {
        MutableBTreeMap::new(self.store_handle().clone())
    }

    fn new_mutable_btree_map_w_values<T, K: Ord, V>(&self, values: T) 
        -> MutableBTreeMap<K, V> 
        where BTreeMap<K, V>: From<T> 
    {
        MutableBTreeMap::new_with_values(values, self.store_handle().clone())
    }
}

pub(crate) trait StoreAccess {    
    
    fn rt(&self) -> &Handle;
    
    fn get_inner(&self) -> &StorePtr;

    /// Attempts to acquire a lock through an `Arc`.
    ///
    /// This function will block the local thread until it is available to acquire
    /// the mutex. Upon returning, the thread is the only thread with the mutex
    /// held. An RAII guard is returned to allow scoped unlock of the lock. When
    /// the guard goes out of scope, the mutex will be unlocked.
    ///
    /// This method requires the `Mutex` to be inside of an `Arc` and the resulting
    /// mutex guard has no lifetime requirements.
    ///
    /// Returned ArcMutexGuard needs to be dropped when done with.
    /// Don't go passing it around causing deadlocks and such.
    fn get_store(&self) -> StoreArcMutexGuard {
        let inner = self.get_inner();
        inner.lock_arc()
    }

    // fn get_store_ptr(&self) -> StorePtr {
    //     self.0.clone()
    // }

    /// Attempts to acquire a lock through an `Arc`.
    ///
    /// If the lock could not be acquired at this time, then `None` is returned.
    /// Otherwise, an RAII guard is returned. The lock will be unlocked when the
    /// guard is dropped.
    ///
    /// This method requires the `Mutex` to be inside of an
    /// `Arc` and the resulting mutex guard has no lifetime requirements.
    ///
    /// This function does not block.
    fn try_get_store(&self) -> Option<StoreArcMutexGuard> {
        let inner = self.get_inner();
        inner.try_lock_arc()
    }

    /// Attempts to acquire this lock through an `Arc` until a timeout is reached.
    ///
    /// If the lock could not be acquired before the timeout expired, then
    /// `None` is returned. Otherwise, an RAII guard is returned. The lock will
    /// be unlocked when the guard is dropped.
    ///
    /// This method requires the `Mutex` to be inside of an
    /// `Arc` and the resulting mutex guard has no lifetime requirements.
    ///
    /// Returned ArcMutexGuard needs to be dropped when done with.
    /// Don't go passing it around causing deadlocks and such.
    fn try_get_store_timeout(&self, duration: Duration) -> Option<StoreArcMutexGuard> {
        let inner = self.get_inner();
        inner.try_lock_arc_for(duration)
    }
    
    fn spawn_fut<F>(&mut self, provider_key: Option<SpawnedFutureKey>, f: F)
                               -> SpawnedFutureKey
        where
            F: Future<Output = ()> + Send + 'static
    {
        let mut lock = self.get_store();
        let token = if let Some(p) = provider_key {
            lock.spawned_futs.get(p).unwrap().child_token()
        } else {
            CancellationToken::new()
        };

        let cloned_token = token.clone();
        let _ = self.rt().spawn(async move {
            select! {
                _ = cloned_token.cancelled() => {}
                _ = f => {}
            }
        });

        lock.spawned_futs.insert(token)
    }
}

#[derive(Debug)]
pub struct RxStore {
    rt: Runtime,
    store: Arc<Mutex<StoreInner>>,
    handle: StoreHandle
}

impl RxStore {
    pub fn new() -> RxStore {
        let rt = Builder::new_multi_thread()
            .build()
            .unwrap();
        
        let store = Arc::new(Mutex::new(StoreInner::new()));
        let handle = StoreHandle::new(rt.handle().clone(), store.clone());
        RxStore {
            rt,
            store,
            handle,
        }
    }
}

impl HasStoreHandle for RxStore {
    fn store_handle(&self) -> &StoreHandle {
        &self.handle
    }
}

impl Manager for RxStore {}

impl StoreAccess for RxStore {
    fn rt(&self) -> &Handle {
        self.rt.handle()
    }

    fn get_inner(&self) -> &Arc<Mutex<StoreInner>> {
        &self.store
    }
}

#[derive(Debug)]
pub(crate) struct StoreInner {
    stored: TypeMap![Send + Sync],
    spawned_futs: SlotMap<SpawnedFutureKey, CancellationToken>
}

impl StoreInner {
    fn new() -> Self {
        Self {
            stored: <TypeMap![Send + Sync]>::new(),
            spawned_futs: SlotMap::default()
        }
    }

    fn set<T: Send + Sync + 'static>(&self, v: T) -> bool {
        self.stored.set(Arc::new(v))
    }

    fn get<T: Send + Sync + 'static>(&self) -> &Arc<T> {
        self
            .stored
            .try_get()
            .expect("state: get() called before set() for given type")
    }
}

#[derive(Debug)]
pub struct StoreHandle {
    rt: Handle,
    store: Arc<Mutex<StoreInner>>
}

impl Clone for StoreHandle {
    fn clone(&self) -> Self {
        StoreHandle {
            rt: self.rt.clone(),
            store: self.store.clone()
        }
    }
}

impl StoreHandle {
    pub(crate) fn new(rt: Handle, store: Arc<Mutex<StoreInner>>) -> Self {
        Self {
            rt,
            store,
        }
    }
}

impl Manager for StoreHandle {}

impl HasStoreHandle for StoreHandle {
    fn store_handle(&self) -> &StoreHandle {
        &self
    }
}

impl StoreAccess for StoreHandle {
    fn rt(&self) -> &Handle {
        &self.rt
    }

    fn get_inner(&self) -> &Arc<Mutex<StoreInner>> {
        &self.store
    }
}

#[cfg(test)]
mod tests {
    use std::ops::Deref;
    use std::sync::Arc;
    use std::thread;
    use std::time::Duration;
    use crate::observable::{Observe};
    use crate::store::{Manager, RxStore, StoreAccess};

    #[test]
    fn it_gets_parking_lot_lock() {
        let store = RxStore::new();
        assert!(store.try_get_store().is_some())
    }

    #[test]
    fn it_returns_none_when_locked() {
        let store = RxStore::new();
        // Holding lock
        let _lock = store.get_store();
        // None because lock not available
        assert!(store.try_get_store().is_none())
    }

    #[test]
    fn it_returns_none_until_timeout() {
        let store = RxStore::new();
        let duration = Duration::from_millis(1000);
        {
            let lock = store.get_store();
            assert!(store.try_get_store_timeout(duration).is_none());
        }
        assert!(store.try_get_store_timeout(duration).is_some())
    }

    #[derive(PartialEq, Debug, Copy, Clone)]
    struct TestTypeOne(i32);

    #[test]
    fn it_gets_type() {
        let store = RxStore::new();
        let one_a = TestTypeOne(50);
        assert!(store.set(one_a));
        let one_b = store.get::<Arc<TestTypeOne>>();
        assert_eq!(one_a, **one_b)
    }

    #[test]
    fn it_creates_mutable() {
        let store = RxStore::new();
        let mutable = store.new_mutable(50);
        assert_eq!(mutable.get(), 50)
    }

    #[test]
    fn it_derives_observable_from_mutable() {
        let store = RxStore::new();
        let mutable = store.new_mutable(50);
        let observable = mutable.observe(|source| source + 20 );
        mutable.set(100);

        thread::sleep(Duration::from_millis(500));

        assert_eq!(observable.get(), 120)
    }

    #[test]
    fn it_derives_observable_cloned_from_mutable() {
        let store = RxStore::new();
        let mutable = store.new_mutable("hello".to_string());
        let observable = mutable.deref().observe(|source| {
            format!("{}, general kenobi", source)
        });
        mutable.set(format!("{} there", mutable.get_cloned()));

        thread::sleep(Duration::from_millis(500));

        assert_eq!(observable.get_cloned(), "hello there, general kenobi".to_string())
    }

    #[test]
    fn it_derives_observable_from_observable() {
        let store = RxStore::new();
        let mutable = store.new_mutable(50);
        let observable_one = mutable.observe(|source| source + 20);
        let observable_two = observable_one.observe(|source| source + 30);

        mutable.set(100);
        thread::sleep(Duration::from_millis(500));
        assert_eq!(observable_two.get(), 150);
    }

    #[test]
    fn it_derives_observable_cloned_from_observable() {
        let store = RxStore::new();
        let mutable = store.new_mutable("hello".to_string());
        let observable_one = mutable.observe(|source| {
            format!("{}, general kenobi", source)
        });
        let observable_two = observable_one.observe(|source| {
            format!("{}, nice to see you", source)
        });

        mutable.set(format!("{} there", mutable.get_cloned()));
        thread::sleep(Duration::from_millis(500));
        assert_eq!(observable_two.get_cloned(), "hello there, general kenobi, nice to see you".to_string())
    }
}

// /// Controls access to internal store by controlling
// /// access to mutex lock
// #[derive(Debug)]
// pub struct RxStoreManager(StorePtr);
// 
// impl RxStoreManager {
// 
//     pub fn new() -> Self {
//         Self(Arc::new(Mutex::new(RxStore::new())))
//     }
// 
//     /// Attempts to acquire a lock through an `Arc`.
//     ///
//     /// This function will block the local thread until it is available to acquire
//     /// the mutex. Upon returning, the thread is the only thread with the mutex
//     /// held. An RAII guard is returned to allow scoped unlock of the lock. When
//     /// the guard goes out of scope, the mutex will be unlocked.
//     ///
//     /// This method requires the `Mutex` to be inside of an `Arc` and the resulting
//     /// mutex guard has no lifetime requirements.
//     ///
//     /// Returned ArcMutexGuard needs to be dropped when done with.
//     /// Don't go passing it around causing deadlocks and such.
//     pub fn get_store(&self) -> StoreArcMutexGuard {
//         let inner = &self.0;
//         inner.lock_arc()
//     }
// 
//     pub(crate) fn get_store_ptr(&self) -> StorePtr {
//         self.0.clone()
//     }
// 
//     /// Attempts to acquire a lock through an `Arc`.
//     ///
//     /// If the lock could not be acquired at this time, then `None` is returned.
//     /// Otherwise, an RAII guard is returned. The lock will be unlocked when the
//     /// guard is dropped.
//     ///
//     /// This method requires the `Mutex` to be inside of an
//     /// `Arc` and the resulting mutex guard has no lifetime requirements.
//     ///
//     /// This function does not block.
//     pub fn try_get_store(&self) -> Option<StoreArcMutexGuard> {
//         let inner = &self.0;
//         inner.try_lock_arc()
//     }
// 
//     /// Attempts to acquire this lock through an `Arc` until a timeout is reached.
//     ///
//     /// If the lock could not be acquired before the timeout expired, then
//     /// `None` is returned. Otherwise, an RAII guard is returned. The lock will
//     /// be unlocked when the guard is dropped.
//     ///
//     /// This method requires the `Mutex` to be inside of an
//     /// `Arc` and the resulting mutex guard has no lifetime requirements.
//     ///
//     /// Returned ArcMutexGuard needs to be dropped when done with.
//     /// Don't go passing it around causing deadlocks and such.
//     pub fn try_get_store_timeout(&self, duration: Duration) -> Option<StoreArcMutexGuard> {
//         let inner = &self.0;
//         inner.try_lock_arc_for(duration)
//     }
// 
//     pub fn get_child_cancellation_token<S>(&self, source: &S) -> Result<CancellationToken, &'static str>
//         where S: HasSpawnedFutureKey 
//     {
//         let lock = self.get_store();
//         let key = lock.spawned_futs
//             .get(source.spawned_future_key())
//             .ok_or("No associated token found for source")?;
//         Ok(key.child_token())    
//     }
// 
//     pub fn register_effect<T, S, F>(&self, source: &S, f: F) -> Result<SpawnedFutureKey, &'static str>
//         where 
//             T: Copy,
//             S: HasSpawnedFutureKey + HasSignal<T> + Clone,
//             <S as HasSignal<T>>::Return: Signal + Send + 'static,
//             F: Fn(<<S as HasSignal<T>>::Return as Signal>::Item) + Send + 'static
//     {
//         let source_cloned = source.clone();
//         let fut = source_cloned.signal().for_each(move |v| {
//             f(v);
//             async {}
//         });
// 
//         let mut lock = self.get_store();
//         let key = lock.spawn_fut(Some(source.spawned_future_key()), fut);
//         Ok(key)
//     }
//     
//     // pub fn create_mutable<T: 'static>(&self, v: T) -> Mutable<T> {
//     //     Mutable::new(v)
//     // }
// 
//     // pub fn derive_observable<U, A, S, F>(
//     //     &self,
//     //     source: &S,
//     //     f: F
//     // ) -> Observable<U>
//     //     where
//     //         A: Copy + Send + Sync + 'static,
//     //         U: Default + Send + Sync + 'static,
//     //         S: Clone + HasSignal<A> + Send + Sync + 'static,
//     //         <S as HasSignal<A>>::Return: crate::signal::Signal + Send + Sync + 'static,
//     //         F: Fn(<<S as HasSignal<A>>::Return as crate::signal::Signal>::Item) -> U + Send + Sync + 'static
//     // {
//     //     let out = self.create_mutable(U::default());
//     //     let out_mutable_clone = out.clone();
//     //     let fut = source.signal().for_each(move |v| {
//     //         out_mutable_clone.set(f(v));
//     //         async {  }
//     //     });
//     // 
//     //     let mut lock = self.get_store();
//     //     let fut_key = lock.spawn_fut(None, fut);
//     //     Observable {
//     //         mutable: out,
//     //         fut_key
//     //     }
//     // }
// 
//     // pub fn derive_observable_cloned<U, A, S, F>(
//     //     &self,
//     //     source: &S,
//     //     f: F
//     // ) -> Observable<U>
//     //     where
//     //         A: Clone + SSS,
//     //         U: Default + SSS,
//     //         S: Clone + HasSignalCloned<A> + SSS,
//     //         <S as HasSignalCloned<A>>::Return: crate::signal::Signal + SSS,
//     //         F: Fn(<<S as HasSignalCloned<A>>::Return as crate::signal::Signal>::Item) -> U + SSS
//     // {
//     //     let out = self.create_mutable(U::default());
//     //     let out_mutable_clone = out.clone();
//     //     let fut = source.signal_cloned().for_each(move |v| {
//     //         out_mutable_clone.set(f(v));
//     //         async {  }
//     //     });
//     // 
//     //     let mut lock = self.get_store();
//     //     let fut_key = lock.spawn_fut(None, fut);
//     //     Observable {
//     //         mutable: out,
//     //         fut_key
//     //     }
//     // }
// }
// 
// // pub enum ValueState<T> {
// //     Active(T),
// //     Cancelled(T)
// // }
// 
// #[derive(Debug)]
// pub struct RxStore {
//     rt: Runtime,
//     stored: TypeMap![Send + Sync],
//     spawned_futs: SlotMap<SpawnedFutureKey, CancellationToken>
// }
// 
// impl RxStore {
// 
//     fn new() -> Self {
//         let rt = Builder::new_multi_thread()
//             .build()
//             .unwrap();
//         Self {
//             rt,
//             stored: <TypeMap![Send + Sync]>::new(),
//             spawned_futs: SlotMap::default()
//         }
//     }
// 
//     // pub(crate) fn create_mutable<T: 'static>(&self, v: T) -> Mutable<T> {
//     //     Mutable::new(v)
//     // }
// 
//     pub fn set<T: Send + Sync + 'static>(&self, v: T) -> bool {
//         self.stored.set(Arc::new(v))
//     }
// 
//     // pub fn set_node<T, U>(&self, v: T) -> bool where T: Send + Sync + Into<NodeType<U>> + 'static {
//     //     self.stored.set(v)
//     // }
// 
//     pub fn get<T: Send + Sync + 'static>(&self) -> &Arc<T> {
//         self
//             .stored
//             .try_get()
//             .expect("state: get() called before set() for given type")
//     }
// 
//     pub(crate) fn spawn_fut<F>(&mut self, provider_key: Option<SpawnedFutureKey>, f: F)
//                                -> SpawnedFutureKey
//         where
//             F: Future<Output = ()> + Send + 'static
//     {
//         let token = if let Some(p) = provider_key {
//             self.spawned_futs.get(p).unwrap().child_token()
//         } else {
//             CancellationToken::new()
//         };
// 
//         let cloned_token = token.clone();
//         let h = self.rt.spawn(async move {
//             select! {
//                 _ = cloned_token.cancelled() => {}
//                 _ = f => {}
//             }
//         });
// 
//         self.spawned_futs.insert(token)
//     }
// 
//     pub(crate) fn clean_up(&mut self, s: SpawnedFutureKey) {
//         if let Some(v) = self.spawned_futs.get(s) { v.cancel() }
//         self.spawned_futs.remove(s);
//     }
// }