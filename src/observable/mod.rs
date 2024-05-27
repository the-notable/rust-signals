use crate::signal::{Mutable, MutableSignal, MutableSignalCloned};
use crate::store::SpawnedFutKey;
use crate::traits::{HasSignal, HasSignalCloned};

#[derive(Debug, Clone)]
pub struct Observable<T> {
    pub(crate) mutable: Mutable<T>,
    pub(crate) fut_key: SpawnedFutKey
}

impl<T: Copy> Observable<T> {
    pub fn get(&self) -> T {
        self.mutable.get()
    }
}

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