use crate::signal::Mutable;
use crate::store::SpawnedFutKey;

#[derive(Debug)]
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