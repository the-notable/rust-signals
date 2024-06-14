use std::collections::BTreeMap;
use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use parking_lot::{Mutex, RawMutex};
use parking_lot::lock_api::ArcMutexGuard;
use slotmap::{new_key_type, SlotMap};
use state::TypeMap;
use tokio::runtime::{Builder as TokioBuilder, Handle, Runtime};
use tokio::select;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;
use vec1::Vec1;

use crate::signal::{ComposableBuilder, ComposableMapBuilder, Mutable};
use crate::signal_map::MutableBTreeMap;
use crate::signal_vec::MutableVec;
use crate::traits::HasStoreHandle;

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

    fn get<T: Send + Sync + 'static>(&self) -> Option<Arc<T>> {
        let lock = self.store_handle().get_store();
        lock.get::<Arc<T>>().cloned()
    }
    
    fn new_composable_with<T: Default, F, O>(&self, f: F) -> ComposableBuilder<T>
        where
            F: Fn(Mutable<T>) -> O,
            O: Future<Output=()> + Send + 'static
    {
        ComposableBuilder::new_with(self.store_handle().clone(), f)
    }

    fn new_composable_map_with<K, V, F, O>(&self, f: F) -> ComposableMapBuilder<K, V>
        where
            K: Ord + Clone,
            V: Clone,
            F: Fn(MutableBTreeMap<K, V>) -> O,
            O: Future<Output=()> + Send + 'static
    {
        ComposableMapBuilder::new_with(self.store_handle().clone(), f)
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
    
    fn spawn_fut<F>(
        &mut self, 
        provider_key: Option<SpawnedFutureKey>, 
        f: F
    ) 
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
        self.rt().spawn(async move {
            select! {
                _ = cloned_token.cancelled() => {}
                _ = f => {}
            }
        });

        lock.spawned_futs.insert(token)
    }

    /// This method returns a token which will be canceled
    /// whenever any one of the cancellation tokens associated with
    /// the provided keys is cancelled.
    /// 
    /// If an empty Vec of keys is provided this method will still 
    /// return a 
    fn derive_dependent_cancellation_token(
        &mut self, 
        provider_keys: Vec1<SpawnedFutureKey>
    ) -> SpawnedFutureKey 
    {
        let token = CancellationToken::new();
        let mut lock = self.get_store();
        let tokens: Vec<CancellationToken> = provider_keys
            .iter()
            .map(|key| {
                lock.spawned_futs.get(*key).unwrap().child_token()
            })
            .collect();

        let mut set = JoinSet::new();
        tokens
            .into_iter()
            .for_each(|v| {
                set.spawn_on(v.cancelled_owned(), self.rt());
            });

        let cloned_token = token.clone();
        self.rt().spawn(async move {
            let _ = set.join_next().await;
            cloned_token.cancel()
        });

        lock.spawned_futs.insert(token)
    }
    
    // fn spawn_futs<F>(&mut self, provider_keys: Vec<SpawnedFutureKey>, f: F)
    //                 -> SpawnedFutureKey
    //     where
    //         F: Future<Output = ()> + Send + 'static
    // {
    //     let token = CancellationToken::new();
    //     if !provider_keys.is_empty() {
    //         // makes new token dependent on cancellation of 
    //         // "parent" futures
    //         let lock = self.get_store();
    //         let tokens: Vec<CancellationToken> = provider_keys
    //             .iter()
    //             .map(|key| {
    //                 lock.spawned_futs.get(*key).unwrap().child_token()
    //             })
    //             .collect();
    // 
    //         let mut set = JoinSet::new();
    //         tokens
    //             .into_iter()
    //             .for_each(|v| { 
    //                 set.spawn_on(v.cancelled_owned(), self.rt()); 
    //             });
    // 
    //         let cloned_token = token.clone();
    //         self.rt().spawn(async move {
    //             let _ = set.join_next().await;
    //             cloned_token.cancel()
    //         });
    //     }
    //     
    //     let cloned_token = token.clone();
    //     self.rt().spawn(async move {
    //         select! {
    //             _ = cloned_token.cancelled() => {}
    //             _ = f => {}
    //         }
    //     });
    // 
    //     let mut lock = self.get_store();
    //     lock.spawned_futs.insert(token)
    // }
}

#[derive(Debug)]
pub struct RxStore {
    rt: Runtime,
    store: Arc<Mutex<StoreInner>>,
    handle: StoreHandle
}

impl RxStore {
    pub fn new() -> RxStore {
        Self::default()
    }
}

impl Default for RxStore {
    fn default() -> Self {
        let rt = TokioBuilder::new_multi_thread()
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
        self.stored.set(v)
    }

    fn get<T: Send + Sync + 'static>(&self) -> Option<&T> {
        self
            .stored
            .try_get::<T>()
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
        self
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
    use std::thread;
    use std::time::Duration;
    use vec1::vec1;

    use crate::signal::{ObserveSignal, SignalExt};
    use crate::store::{Manager, RxStore, StoreAccess};
    use crate::traits::HasSignal;

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
            let _lock = store.get_store();
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
        let one_b = store.get::<TestTypeOne>().unwrap();
        assert_eq!(one_a, *one_b)
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
        let observable = mutable
            .signal()
            .map(|v| v + 20)
            .observe();
        mutable.set(100);

        thread::sleep(Duration::from_millis(500));

        assert_eq!(observable.get(), 120)
    }

    #[test]
    fn it_derives_observable_from_map_signal() {
        let store = RxStore::new();
        let mutable = store.new_mutable(50);
        
        let observable = mutable
            .signal()
            .map(|v| v + 20)
            .observe();
        
        //let observable = mutable.observe(|source| source + 20 );
        mutable.set(100);

        thread::sleep(Duration::from_millis(500));

        assert_eq!(observable.get(), 120)
    }

    #[test]
    fn it_derives_observable_from_inspect_signal() {
        let store = RxStore::new();
        let mutable = store.new_mutable(50);

        let observable = mutable
            .signal()
            .map(|v| v + 20)
            .observe();

        //let observable = mutable.observe(|source| source + 20 );
        mutable.set(100);

        thread::sleep(Duration::from_millis(500));

        assert_eq!(observable.get(), 120)
    }
    
    #[test]
    fn it_derives_observable_cloned_from_mutable() {
        let store = RxStore::new();
        let mutable = store.new_mutable("hello".to_string());
        let observable = mutable
            .signal()
            .map(|v| format!("{}, general kenobi", v))
            .observe();
        mutable.set(format!("{} there", mutable.get_cloned()));

        thread::sleep(Duration::from_millis(500));

        assert_eq!(observable.get_cloned(), "hello there, general kenobi".to_string())
    }

    #[test]
    fn it_derives_observable_from_observable() {
        let store = RxStore::new();
        let mutable = store.new_mutable(50);
        let observable_one = mutable
            .signal()
            .map(|v| v + 20)
            .observe();
        
        let observable_two = observable_one
            .signal()
            .map(|v| v + 30)
            .observe();

        mutable.set(100);
        thread::sleep(Duration::from_millis(500));
        assert_eq!(observable_two.get(), 150);
    }

    #[test]
    fn it_derives_observable_cloned_from_observable() {
        let store = RxStore::new();
        let mutable = store.new_mutable("hello".to_string());

        let observable_one = mutable
            .signal()
            .map(|v| format!("{}, general kenobi", v))
            .observe();

        let observable_two = observable_one
            .signal()
            .map(|v| format!("{}, nice to see you", v))
            .observe();

        mutable.set(format!("{} there", mutable.get_cloned()));
        thread::sleep(Duration::from_millis(500));
        assert_eq!(observable_two.get_cloned(), "hello there, general kenobi, nice to see you".to_string())
    }
    
    #[test]
    fn it_combines_cancellation_tokens() {
        let mut store = RxStore::new();
        
        let key_1_1 = store.spawn_fut(None, async {});
        
        let key_2_1= store.spawn_fut(None, async {});
        let key_2_2_dep_on_1_1 = store.spawn_fut(Some(key_1_1), async {});
        let key_2_3 = store.spawn_fut(None, async {});
        
        let keys = vec1![key_2_1, key_2_2_dep_on_1_1, key_2_3];
        let joined_key = store.derive_dependent_cancellation_token(keys);
        
        let store_lock = store.store.lock();
        let joined_token = store_lock.spawned_futs.get(joined_key).unwrap();
        assert!(!joined_token.is_cancelled());
        
        let key_1_1_token = store_lock.spawned_futs.get(key_1_1).unwrap();
        key_1_1_token.cancel();
        
        thread::sleep(Duration::from_millis(500));
        assert!(key_1_1_token.is_cancelled());
        assert!(joined_token.is_cancelled());

        let key_2_1_token = store_lock.spawned_futs.get(key_2_1).unwrap();
        assert!(!key_2_1_token.is_cancelled());
    }
}