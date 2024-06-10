use std::future::Future;
use has_store_handle_macro::has_store_handle;
use crate::signal::Mutable;
use crate::store::{SpawnedFutureKey, StoreAccess};
use crate::traits::HasStoreHandle;
use crate::store::StoreHandle;

#[derive(Debug)]
#[has_store_handle]
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

#[derive(Debug)]
#[has_store_handle]
pub struct Composable<A> {
    inner: Mutable<A>,
    fut_keys: Vec<SpawnedFutureKey>
}
