use std::future::Future;
use std::sync::Arc;
use std::time::Duration;
use parking_lot::{ArcMutexGuard, Mutex, RawMutex};
use slotmap::{new_key_type, SlotMap};
use state::TypeMap;
use tokio::{select, spawn};
use tokio_util::sync::CancellationToken;
use crate::signal::Mutable;

new_key_type! {
    pub struct SpawnedFutKey;
}

/// Controls access to internal store by controlling
/// access to mutex lock
pub struct RxStoreManager(Arc<Mutex<RxStore>>);

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
        Mutable::new(v, self.0.clone())
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

    pub(crate) fn create_mutable<T: 'static>(&self, v: T, store_ref: Arc<Mutex<RxStore>>) -> Mutable<T> {
        Mutable::new(v, store_ref)
    }

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
        spawn(async move {
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