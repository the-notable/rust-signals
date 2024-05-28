pub trait SSS: Send + Sync + 'static {}
impl<T: Send + Sync + 'static> SSS for T {}

pub trait Get<T: Copy> {
    fn get(&self) -> T;
}

pub trait GetCloned<T: Clone> {
    fn get_cloned(&self) -> T;
}

pub trait HasSignal<T: Copy> {
    type Return;//: Signal + Send + Sync + 'static;

    fn signal(&self) -> Self::Return;
}

pub trait HasSignalCloned<T: Clone> {
    type Return;//: Signal + Send + Sync + 'static;

    fn signal_cloned(&self) -> Self::Return;
}