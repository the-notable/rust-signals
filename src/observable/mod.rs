use crate::signal::Mutable;
use crate::store::SpawnedFutKey;

#[derive(Debug)]
pub struct Observable<T> {
    pub(crate) mutable: Mutable<T>,
    pub(crate) fut_key: SpawnedFutKey
}