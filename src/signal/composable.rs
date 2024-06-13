use std::future::Future;
use vec1::Vec1;
use has_store_handle_macro::has_store_handle;
use crate::signal::{Mutable, MutableLockRef, MutableSignal, SignalExt};
use crate::store::{SpawnedFutureKey, StoreAccess};
use crate::traits::{Get, HasSignal, HasStoreHandle, Provider, SSS};
use crate::store::StoreHandle;

#[has_store_handle]
#[derive(Debug)]
pub struct ComposableBuilder<A> {
    inner: Mutable<A>,
    fut_keys: Vec<SpawnedFutureKey>
}

impl<A: Default> ComposableBuilder<A> {
    pub(crate) fn new_with<F, O>(
        store_handle: StoreHandle,
        f: F
    )
        -> Self
        where
            F: Fn(Mutable<A>) -> O,
            O: Future<Output=()> + Send + 'static
    {
        let mut builder = Self {
            inner: Mutable::new(A::default(), store_handle.clone()),
            fut_keys: vec![],
            store_handle
        };

        let inner_clone = builder.inner.clone();
        let fut = f(inner_clone);
        let fut_key = builder.store_handle.spawn_fut(None, fut);
        builder.fut_keys.push(fut_key);

        builder
    }

    pub fn and_with<F, O>(mut self, f: F) -> Self
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
    
    pub fn build(mut self) -> Composable<A> {
        let fut_key = self
            .store_handle
            .derive_dependent_cancellation_token(Vec1::try_from_vec(self.fut_keys).unwrap());
        
        Composable {
            inner: self.inner,
            fut_key,
            store_handle: self.store_handle,
        }
    }
}

#[has_store_handle]
#[derive(Debug)]
pub struct Composable<A> {
    inner: Mutable<A>,
    fut_key: SpawnedFutureKey
}

impl<A: Clone + SSS> Provider for Composable<A> {
    type YieldedValue = A;

    fn fut_key(&self) -> Option<SpawnedFutureKey> {
        Some(self.fut_key)
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