use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use parking_lot::{ArcMutexGuard, Mutex, RawMutex};
use slotmap::{new_key_type, SlotMap};
use state::TypeMap;
use tokio::{select, spawn};
use tokio_util::sync::CancellationToken;

use crate::observable::Observable;
use crate::signal::Mutable;
use crate::signal::SignalExt;
use crate::traits::{HasSignal, HasSignalCloned};

new_key_type! {
    pub struct SpawnedFutKey;
}

pub(crate) type StoreRef = Arc<Mutex<RxStore>>;

/// Controls access to internal store by controlling
/// access to mutex lock
pub struct RxStoreManager(StoreRef);

impl RxStoreManager {

    pub fn new() -> Self {
        Self(Arc::new(Mutex::new(RxStore::new())))
    }

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
    pub(crate) fn get_store(&self) -> ArcMutexGuard<RawMutex, RxStore> {
        self.0.lock_arc()
    }

    pub(crate) fn get_store_ptr(&self) -> Arc<Mutex<RxStore>> {
        self.0.clone()
    }

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
    pub fn try_get_store(&self) -> Option<ArcMutexGuard<RawMutex, RxStore>> {
        self.0.try_lock_arc()
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
    pub fn try_get_store_timeout(&self, duration: Duration) -> Option<ArcMutexGuard<RawMutex, RxStore>> {
        self.0.try_lock_arc_for(duration)
    }

    pub fn create_mutable<T: 'static>(&self, v: T) -> Mutable<T> {
        Mutable::new(v)
    }

    pub fn derive_observable<U, A, S, F>(
        &self,
        source: &S,
        f: F
    ) -> Observable<U>
        where
            A: Copy + Send + Sync + 'static,
            U: Default + Send + Sync + 'static,
            S: Clone + HasSignal<A> + Send + Sync + 'static,
            <S as HasSignal<A>>::Return: crate::signal::Signal + Send + Sync + 'static,
            F: Fn(<<S as HasSignal<A>>::Return as crate::signal::Signal>::Item) -> U + Send + Sync + 'static
    {
        let out = self.create_mutable(U::default());
        let out_mutable_clone = out.clone();
        let fut = source.signal().for_each(move |v| {
            out_mutable_clone.set(f(v));
            async {  }
        });

        let mut lock = self.get_store();
        let fut_key = lock.spawn_fut(None, fut);
        Observable {
            mutable: out,
            fut_key
        }
    }

    pub fn derive_observable_cloned<U, A, S, F>(
        &self,
        source: &S,
        f: F
    ) -> Observable<U>
        where
            A: Clone + Send + Sync + 'static,
            U: Default + Send + Sync + 'static,
            S: Clone + HasSignalCloned<A> + Send + Sync + 'static,
            <S as HasSignalCloned<A>>::Return: crate::signal::Signal + Send + Sync + 'static,
            F: Fn(<<S as HasSignalCloned<A>>::Return as crate::signal::Signal>::Item) -> U + Send + Sync + 'static
    {
        let out = self.create_mutable(U::default());
        let out_mutable_clone = out.clone();
        let fut = source.signal_cloned().for_each(move |v| {
            out_mutable_clone.set(f(v));
            async {  }
        });

        let mut lock = self.get_store();
        let fut_key = lock.spawn_fut(None, fut);
        Observable {
            mutable: out,
            fut_key
        }
    }
}




pub struct RxStore {
    stored: TypeMap![Send + Sync],
    spawned_futs: SlotMap<SpawnedFutKey, CancellationToken>
}

impl RxStore {

    fn new() -> Self {
        Self {
            stored: <TypeMap![Send + Sync]>::new(),
            spawned_futs: SlotMap::default()
        }
    }

    // pub(crate) fn create_mutable<T: 'static>(&self, v: T) -> Mutable<T> {
    //     Mutable::new(v)
    // }

    pub fn set<T: Send + Sync + 'static>(&self, v: T) -> bool {
        self.stored.set(v)
    }

    // pub fn set_node<T, U>(&self, v: T) -> bool where T: Send + Sync + Into<NodeType<U>> + 'static {
    //     self.stored.set(v)
    // }

    pub fn get<T: Send + Sync + 'static>(&self) -> &T {
        self
            .stored
            .try_get()
            .expect("state: get() called before set() for given type")
    }

    pub(crate) fn spawn_fut<F>(&mut self, provider_key: Option<SpawnedFutKey>, f: F)
                               -> SpawnedFutKey
        where
            F: Future<Output = ()> + Send + 'static
    {
        let token = if let Some(p) = provider_key {
            self.spawned_futs.get(p).unwrap().child_token()
        } else {
            CancellationToken::new()
        };

        let cloned_token = token.clone();
        let h = spawn(async move {
            select! {
                _ = cloned_token.cancelled() => {}
                _ = f => {}
            }
        });

        let key = self.spawned_futs.insert(token);
        key
    }

    pub(crate) fn clean_up(&mut self, s: SpawnedFutKey) {
        if let Some(v) = self.spawned_futs.get(s) { v.cancel() }
        self.spawned_futs.remove(s);
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use crate::store::{RxStore, RxStoreManager};

    #[test]
    fn it_gets_parking_lot_lock() {
        let store = RxStoreManager::new();
        assert!(store.try_get_store().is_some())
    }

    #[test]
    fn it_returns_none_when_locked() {
        let store = RxStoreManager::new();
        // Holding lock
        let _lock = store.get_store();
        // None because lock not available
        assert!(store.try_get_store().is_none())
    }

    #[test]
    fn it_returns_none_until_timeout() {
        let store = RxStoreManager::new();
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
        let s = RxStore::new();
        let one_a = TestTypeOne(50);
        s.set(one_a);
        let one_b = s.get::<TestTypeOne>();
        assert_eq!(one_a, *one_b)
    }

    #[test]
    fn it_creates_mutable() {
        let store = RxStoreManager::new();
        let mutable = store.create_mutable(50);
        assert_eq!(mutable.get(), 50)
    }

    #[tokio::test]
    async fn it_derives_observable_from_mutable() {
        let store = RxStoreManager::new();
        let mutable = store.create_mutable(50);
        let mutable_clone = mutable.clone();
        let observable = store.derive_observable(&*mutable_clone, |source| source + 20 );
        mutable.set(100);

        tokio::time::sleep(Duration::from_millis(500)).await;

        assert_eq!(observable.get(), 120)
    }

    #[tokio::test]
    async fn it_derives_observable_cloned_from_mutable() {
        let store = RxStoreManager::new();
        let mutable = store.create_mutable("hello".to_string());
        let mutable_clone = mutable.clone();
        let observable = store.derive_observable_cloned(&*mutable_clone, |source| {
            format!("{}, general kenobi", source)
        });
        mutable.set(format!("{} there", mutable.get_cloned()));

        tokio::time::sleep(Duration::from_millis(500)).await;

        assert_eq!(observable.get_cloned(), "hello there, general kenobi".to_string())
    }

    #[tokio::test]
    async fn it_derives_observable_from_observable() {
        let store = RxStoreManager::new();
        let mutable = store.create_mutable(50);
        let mutable_clone = mutable.clone();
        let observable_one = store.derive_observable(&*mutable_clone, |source| source + 20);
        let observable_two = store.derive_observable(&observable_one, |source| source + 30);
        mutable.set(100);

        tokio::time::sleep(Duration::from_millis(500)).await;

        assert_eq!(observable_two.get(), 150)
    }

    #[tokio::test]
    async fn it_derives_observable_cloned_from_observable() {
        let store = RxStoreManager::new();
        let mutable = store.create_mutable("hello".to_string());
        let mutable_clone = mutable.clone();
        let observable_one = store.derive_observable_cloned(&*mutable_clone, |source| {
            format!("{}, general kenobi", source)
        });
        let observable_two = store.derive_observable_cloned(&observable_one, |source| {
            format!("{}, nice to see you", source)
        });
        mutable.set(format!("{} there", mutable.get_cloned()));

        tokio::time::sleep(Duration::from_millis(500)).await;

        assert_eq!(observable_two.get_cloned(), "hello there, general kenobi, nice to see you".to_string())
    }
}