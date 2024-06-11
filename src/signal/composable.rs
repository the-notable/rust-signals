use std::future::Future;
use has_store_handle_macro::has_store_handle;
use crate::signal::{Mutable, MutableLockRef, MutableSignal};
use crate::store::{SpawnedFutureKey, StoreAccess};
use crate::traits::{Get, HasSignal, HasStoreHandle};
use crate::store::StoreHandle;

#[has_store_handle]
#[derive(Debug)]
pub struct ComposableBuilder<A> {
    inner: Mutable<A>,
    fut_keys: Vec<SpawnedFutureKey>
}

impl<A: Default> ComposableBuilder<A> {
    pub(crate) fn new(store_handle: StoreHandle) -> Self {
        Self {
            inner: Mutable::new(A::default(), store_handle.clone()),
            fut_keys: vec![],
            store_handle
        }
    }

    pub fn with<F, O>(mut self, f: F) -> Self 
        where 
            F: Fn(Mutable<A>) -> O,
            O: Future<Output=()> + Send + 'static
    {
        let inner_clone = self.inner.clone();
        let fut = f(inner_clone);
        let fut_key = self.store_handle.spawn_fut(None, fut);
        self.fut_keys.push(fut_key);
        self
    }
    
    pub fn build(self) -> Composable<A> {
        Composable {
            inner: self.inner,
            fut_keys: self.fut_keys,
            store_handle: self.store_handle,
        }
    }
}

#[has_store_handle]
#[derive(Debug)]
pub struct Composable<A> {
    inner: Mutable<A>,
    fut_keys: Vec<SpawnedFutureKey>
}

impl<A> Composable<A> {
    pub fn lock_ref(&self) -> MutableLockRef<A> {
        self.inner.lock_ref()
    }
}

impl<A: Clone> HasSignal<A> for Composable<A> {
    type Return = MutableSignal<A>;

    fn signal(&self) -> Self::Return {
        self.inner.signal()
    }
}

impl<A: Clone> Get<A> for Composable<A> {
    fn get(&self) -> A {
        self.inner.get()
    }
}