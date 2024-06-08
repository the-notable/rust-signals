use std::cmp::Ord;
use std::collections::{BTreeMap, VecDeque};
use std::future::Future;
use std::marker::Unpin;
use std::pin::Pin;
use std::task::{Context, Poll};

use futures_core::Stream;
use futures_util::stream;
use futures_util::stream::StreamExt;
use pin_project::pin_project;

use crate::signal::Signal;
use crate::signal_map::observable_btree_map::ObservableBTreeMap;
use crate::store::{Manager, StoreAccess, StoreHandle};
use crate::traits::{HasStoreHandle, SSS};

pub use self::mutable_btree_map::*;

// TODO make this non-exhaustive
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum MapDiff<K, V> {
    Replace {
        entries: Vec<(K, V)>,
    },

    Insert {
        key: K,
        value: V,
    },

    Update {
        key: K,
        value: V,
    },

    Remove {
        key: K,
    },

    Clear {},
}

impl<K, A> MapDiff<K, A> {
    // TODO inline this ?
    fn map<B, F>(self, mut callback: F) -> MapDiff<K, B> where F: FnMut(A) -> B {
        match self {
            // TODO figure out a more efficient way of implementing this
            MapDiff::Replace { entries } => MapDiff::Replace { entries: entries.into_iter().map(|(k, v)| (k, callback(v))).collect() },
            MapDiff::Insert { key, value } => MapDiff::Insert { key, value: callback(value) },
            MapDiff::Update { key, value } => MapDiff::Update { key, value: callback(value) },
            MapDiff::Remove { key } => MapDiff::Remove { key },
            MapDiff::Clear {} => MapDiff::Clear {},
        }
    }
}


// TODO impl for AssertUnwindSafe ?
#[must_use = "SignalMaps do nothing unless polled"]
pub trait SignalMap {
    type Key;
    type Value;

    fn poll_map_change(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<MapDiff<Self::Key, Self::Value>>>;
}


// Copied from Future in the Rust stdlib
impl<'a, A> SignalMap for &'a mut A where A: ?Sized + SignalMap + Unpin {
    type Key = A::Key;
    type Value = A::Value;

    #[inline]
    fn poll_map_change(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<MapDiff<Self::Key, Self::Value>>> {
        A::poll_map_change(Pin::new(&mut **self), cx)
    }
}

// Copied from Future in the Rust stdlib
impl<A> SignalMap for Box<A> where A: ?Sized + SignalMap + Unpin {
    type Key = A::Key;
    type Value = A::Value;

    #[inline]
    fn poll_map_change(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<MapDiff<Self::Key, Self::Value>>> {
        A::poll_map_change(Pin::new(&mut *self), cx)
    }
}

// Copied from Future in the Rust stdlib
impl<A> SignalMap for Pin<A>
    where A: Unpin + std::ops::DerefMut,
          A::Target: SignalMap {
    type Key = <<A as std::ops::Deref>::Target as SignalMap>::Key;
    type Value = <<A as std::ops::Deref>::Target as SignalMap>::Value;

    #[inline]
    fn poll_map_change(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<MapDiff<Self::Key, Self::Value>>> {
        Pin::get_mut(self).as_mut().poll_map_change(cx)
    }
}

pub trait ObserveSignalMap
    where
        Self: SignalMap + HasStoreHandle + Sized + SSS,
        <Self as SignalMap>::Key: Ord + Clone + SSS,
        <Self as SignalMap>::Value: Clone + SSS
{
    fn observe(self) -> ObservableBTreeMap<<Self as SignalMap>::Key, <Self as SignalMap>::Value> {
        let mut store = self.store_handle().clone();
        let out = store.new_mutable_btree_map();
        let out_clone = out.clone();
        let fut = self.for_each(move |v| {
            match v {
                MapDiff::Replace { entries } => {
                    entries
                        .into_iter()
                        .for_each(|(k, v)| { 
                            out_clone.lock_mut().insert(k, v); 
                        });
                }
                MapDiff::Insert { key, value } |
                MapDiff::Update { key, value } => {
                    out_clone.lock_mut().insert(key, value);
                }
                MapDiff::Remove { key } => { 
                    out_clone.lock_mut().remove(&key).unwrap(); 
                },
                MapDiff::Clear { } => { 
                    out_clone.lock_mut().clear(); 
                }
            };
            async {}
        });

        let fut_key = store.spawn_fut(None, fut);
        ObservableBTreeMap {
            store_handle: store,
            map: out,
            fut_key
        }
    }
}

impl<T> ObserveSignalMap for T
    where
        Self: SignalMap + HasStoreHandle + Sized + SSS,
        <Self as SignalMap>::Key: Ord + Clone + SSS,
        <Self as SignalMap>::Value: Clone + SSS {}

// TODO Seal this
pub trait SignalMapExt: SignalMap {
    #[inline]
    fn map_value<A, F>(self, callback: F) -> MapValue<Self, F>
        where F: FnMut(Self::Value) -> A,
              Self: Sized + HasStoreHandle 
    {
        let store_handle = self.store_handle().clone();
        MapValue {
            signal: self,
            callback,
            store_handle
        }
    }

    #[inline]
    fn map_value_signal<A, F>(self, callback: F) -> MapValueSignal<Self, A, F>
        where A: Signal,
              F: FnMut(Self::Value) -> A,
              Self: Sized + HasStoreHandle 
    {
        let store_handle = self.store_handle().clone();
        MapValueSignal {
            signal: Some(self),
            signals: BTreeMap::new(),
            pending: VecDeque::new(),
            callback,
            store_handle
        }
    }

    /// Returns a signal that tracks the value of a particular key in the map.
    #[inline]
    fn key_cloned(self, key: Self::Key) -> MapWatchKeySignal<Self>
        where Self::Key: PartialEq,
              Self::Value: Clone,
              Self: Sized + HasStoreHandle 
    {
        let store_handle = self.store_handle().clone();
        MapWatchKeySignal {
            signal_map: self,
            watch_key: key,
            first: true,
            store_handle
        }
    }

    #[inline]
    fn for_each<U, F>(self, callback: F) -> ForEach<Self, U, F>
        where U: Future<Output = ()>,
              F: FnMut(MapDiff<Self::Key, Self::Value>) -> U,
              Self: Sized {
        // TODO a little hacky
        ForEach {
            inner: SignalMapStream {
                signal_map: self,
            }.for_each(callback)
        }
    }

    /// A convenience for calling `SignalMap::poll_map_change` on `Unpin` types.
    #[inline]
    fn poll_map_change_unpin(&mut self, cx: &mut Context) -> Poll<Option<MapDiff<Self::Key, Self::Value>>> where Self: Unpin + Sized {
        Pin::new(self).poll_map_change(cx)
    }

    #[inline]
    fn boxed<'a>(self) -> Pin<Box<dyn SignalMap<Key = Self::Key, Value = Self::Value> + Send + 'a>>
        where Self: Sized + Send + 'a {
        Box::pin(self)
    }

    #[inline]
    fn boxed_local<'a>(self) -> Pin<Box<dyn SignalMap<Key = Self::Key, Value = Self::Value> + 'a>>
        where Self: Sized + 'a {
        Box::pin(self)
    }
}

// TODO why is this ?Sized
impl<T: ?Sized> SignalMapExt for T where T: SignalMap {}


/// An owned dynamically typed [`SignalMap`].
///
/// This is useful if you don't know the static type, or if you need
/// indirection.
pub type BoxSignalMap<'a, Key, Value> = Pin<Box<dyn SignalMap<Key = Key, Value = Value> + Send + 'a>>;

/// Same as [`BoxSignalMap`], but without the `Send` requirement.
pub type LocalBoxSignalMap<'a, Key, Value> = Pin<Box<dyn SignalMap<Key = Key, Value = Value> + 'a>>;


#[derive(Debug)]
#[must_use = "SignalMaps do nothing unless polled"]
pub struct Always<A> {
    map: Option<A>,
}

impl<A> Unpin for Always<A> {}

impl<A, K, V> SignalMap for Always<A> where A: IntoIterator<Item = (K, V)> {
    type Key = K;
    type Value = V;

    #[inline]
    fn poll_map_change(mut self: Pin<&mut Self>, _cx: &mut Context) -> Poll<Option<MapDiff<Self::Key, Self::Value>>> {
        match self.map.take() {
            Some(map) => {
                let entries: Vec<(K, V)> = map.into_iter().collect();
                Poll::Ready(Some(MapDiff::Replace { entries }))
            },
            None => {
                Poll::Ready(None)
            },
        }
    }
}

/// Converts an `IntoIterator<Item = (K, V)>` into a `SignalMap<Key = K, Value = V>`.
///
/// This will usually be a `BTreeMap<K, V>` or `Vec<(K, V)>`.
#[inline]
pub fn always<A, K, V>(map: A) -> Always<A> where A: IntoIterator<Item = (K, V)> {
    Always {
        map: Some(map),
    }
}


#[pin_project(project = MapValueProj)]
#[derive(Debug)]
#[must_use = "SignalMaps do nothing unless polled"]
pub struct MapValue<A, B> {
    #[pin]
    signal: A,
    callback: B,
    store_handle: StoreHandle
}

impl<A, B> HasStoreHandle for MapValue<A, B> {
    fn store_handle(&self) -> &StoreHandle {
        &self.store_handle
    }
}

impl<A, B, F> SignalMap for MapValue<A, F>
    where A: SignalMap,
          F: FnMut(A::Value) -> B {
    type Key = A::Key;
    type Value = B;

    // TODO should this inline ?
    #[inline]
    fn poll_map_change(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<MapDiff<Self::Key, Self::Value>>> {
        let MapValueProj { 
            signal, 
            callback,
            store_handle: _ 
        } = self.project();

        signal.poll_map_change(cx).map(|some| some.map(|change| change.map(callback)))
    }
}

// This is an optimization to allow a SignalMap to efficiently "return" multiple MapDiff
// TODO can this be made more efficient ?
// TODO refactor `signal_map`'s and `signal_vec`'s `PendingBuilder` & `unwrap` into common helpers?
struct PendingBuilder<A> {
    first: Option<A>,
    rest: VecDeque<A>,
}

impl<A> PendingBuilder<A> {
    fn new() -> Self {
        Self {
            first: None,
            rest: VecDeque::new(),
        }
    }

    fn push(&mut self, value: A) {
        if self.first.is_none() {
            self.first = Some(value);

        } else {
            self.rest.push_back(value);
        }
    }
}

fn unwrap<A>(x: Poll<Option<A>>) -> A {
    match x {
        Poll::Ready(Some(x)) => x,
        _ => panic!("Signal did not return a value"),
    }
}

#[pin_project(project = MapValueSignalProj)]
#[derive(Debug)]
#[must_use = "SignalMaps do nothing unless polled"]
pub struct MapValueSignal<A, B, F> where A: SignalMap, B: Signal {
    #[pin]
    signal: Option<A>,
    // TODO is there a more efficient way to implement this ?
    signals: BTreeMap<A::Key, Option<Pin<Box<B>>>>,
    pending: VecDeque<MapDiff<A::Key, B::Item>>,
    callback: F,
    store_handle: StoreHandle
}

impl<A, B, F> HasStoreHandle for MapValueSignal<A, B, F> 
    where 
        A: SignalMap, 
        B: Signal 
{
    fn store_handle(&self) -> &StoreHandle {
        &self.store_handle
    }
}

impl<A, B, F> SignalMap for MapValueSignal<A, B, F>
    where A: SignalMap,
          A::Key: Clone + Ord,
          B: Signal,
          F: FnMut(A::Value) -> B {
    type Key = A::Key;
    type Value = B::Item;

    fn poll_map_change(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<MapDiff<Self::Key, Self::Value>>> {
        let MapValueSignalProj { 
            mut signal, 
            signals, 
            pending, 
            callback,
            store_handle: _
        } = self.project();

        if let Some(diff) = pending.pop_front() {
            return Poll::Ready(Some(diff));
        }

        let mut new_pending = PendingBuilder::new();

        let done = loop {
            break match signal.as_mut().as_pin_mut().map(|signal| signal.poll_map_change(cx)) {
                None => {
                    true
                },
                Some(Poll::Ready(None)) => {
                    signal.set(None);
                    true
                },
                Some(Poll::Ready(Some(change))) => {
                    new_pending.push(match change {
                        MapDiff::Replace { entries } => {
                            *signals = BTreeMap::new();

                            MapDiff::Replace {
                                entries: entries.into_iter().map(|(key, value)| {
                                    let mut signal = Box::pin(callback(value));
                                    let poll = (key.clone(), unwrap(signal.as_mut().poll_change(cx)));
                                    signals.insert(key, Some(signal));
                                    poll
                                }).collect()
                            }
                        },

                        MapDiff::Insert { key, value } => {
                            let mut signal = Box::pin(callback(value));
                            let poll = unwrap(signal.as_mut().poll_change(cx));
                            signals.insert(key.clone(), Some(signal));
                            MapDiff::Insert { key, value: poll }
                        },

                        MapDiff::Update { key, value } => {
                            let mut signal = Box::pin(callback(value));
                            let poll = unwrap(signal.as_mut().poll_change(cx));
                            *signals.get_mut(&key).expect("The key is not in the map") = Some(signal);
                            MapDiff::Update { key, value: poll }
                        },

                        MapDiff::Remove { key } => {
                            signals.remove(&key);
                            MapDiff::Remove { key }
                        },

                        MapDiff::Clear {} => {
                            signals.clear();
                            MapDiff::Clear {}
                        },
                    });

                    continue;
                },
                Some(Poll::Pending) => false,
            };
        };

        let mut has_pending = false;

        // TODO ensure that this is as efficient as possible
        // TODO make this more efficient (e.g. using a similar strategy as FuturesUnordered)
        for (key, signal) in signals.into_iter() {
            // TODO use a loop until the value stops changing ?
            match signal.as_mut().map(|s| s.as_mut().poll_change(cx)) {
                Some(Poll::Ready(Some(value))) => {
                    new_pending.push(MapDiff::Update { key: key.clone(), value });
                },
                Some(Poll::Ready(None)) => {
                    *signal = None;
                },
                Some(Poll::Pending) => {
                    has_pending = true;
                },
                None => {},
            }
        }

        if let Some(first) = new_pending.first {
            *pending = new_pending.rest;
            Poll::Ready(Some(first))

        } else if done && !has_pending {
            Poll::Ready(None)

        } else {
            Poll::Pending
        }
    }
}

#[pin_project(project = MapWatchKeySignalProj)]
#[derive(Debug)]
pub struct MapWatchKeySignal<M> where M: SignalMap {
    #[pin]
    signal_map: M,
    watch_key: M::Key,
    first: bool,
    store_handle: StoreHandle
}

impl<M> HasStoreHandle for MapWatchKeySignal<M> where M: SignalMap {
    fn store_handle(&self) -> &StoreHandle {
        &self.store_handle
    }
}

impl<M> Signal for MapWatchKeySignal<M>
    where M: SignalMap,
          M::Key: PartialEq,
          M::Value: Clone {
    type Item = Option<M::Value>;

    fn poll_change(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<Self::Item>> {
        let MapWatchKeySignalProj { 
            mut signal_map, 
            watch_key, 
            first,
            store_handle: _
        } = self.project();

        let mut changed: Option<Option<M::Value>> = None;

        let is_done = loop {
            break match signal_map.as_mut().poll_map_change(cx) {
                Poll::Ready(some) => match some {
                    Some(MapDiff::Replace { entries }) => {
                        changed = Some(
                            entries
                                .into_iter()
                                .find(|entry| entry.0 == *watch_key)
                                .map(|entry| entry.1)
                        );
                        continue;
                    },
                    Some(MapDiff::Insert { key, value }) | Some(MapDiff::Update { key, value }) => {
                        if key == *watch_key {
                            changed = Some(Some(value));
                        }
                        continue;
                    },
                    Some(MapDiff::Remove { key }) => {
                        if key == *watch_key {
                            changed = Some(None);
                        }
                        continue;
                    },
                    Some(MapDiff::Clear {}) => {
                        changed = Some(None);
                        continue;
                    },
                    None => {
                        true
                    },
                },
                Poll::Pending => {
                    false
                },
            }
        };

        if let Some(change) = changed {
            *first = false;
            Poll::Ready(Some(change))

        } else if *first {
            *first = false;
            Poll::Ready(Some(None))

        } else if is_done {
            Poll::Ready(None)

        } else {
            Poll::Pending
        }
    }
}


#[pin_project]
#[derive(Debug)]
#[must_use = "Streams do nothing unless polled"]
struct SignalMapStream<A> {
    #[pin]
    signal_map: A,
}

impl<A: SignalMap> Stream for SignalMapStream<A> {
    type Item = MapDiff<A::Key, A::Value>;

    #[inline]
    fn poll_next(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<Self::Item>> {
        self.project().signal_map.poll_map_change(cx)
    }
}


#[pin_project]
#[derive(Debug)]
#[must_use = "Futures do nothing unless polled"]
pub struct ForEach<A, B, C> {
    #[pin]
    inner: stream::ForEach<SignalMapStream<A>, B, C>,
}

impl<A, B, C> Future for ForEach<A, B, C>
    where A: SignalMap,
          B: Future<Output = ()>,
          C: FnMut(MapDiff<A::Key, A::Value>) -> B {
    type Output = ();

    #[inline]
    fn poll(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        self.project().inner.poll(cx)
    }
}


// TODO verify that this is correct
mod mutable_btree_map {
    use std::cmp::{Ord, Ordering};
    use std::collections::BTreeMap;
    use std::fmt;
    use std::hash::{Hash, Hasher};
    use std::marker::Unpin;
    use std::ops::{Deref, Index};
    use std::pin::Pin;
    use std::sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard};
    use std::task::{Context, Poll};

    use futures_channel::mpsc;
    use futures_util::stream::StreamExt;

    use crate::signal_vec::{SignalVec, VecDiff};
    use crate::store::{SpawnedFutureKey, StoreHandle};
    use crate::traits::{HasSignalMap, HasStoreHandle, Provider};

    use super::{MapDiff, SignalMap, SignalMapExt};

    #[derive(Debug)]
    #[has_store_handle_macro::has_store_handle]
    struct MutableBTreeState<K, V> {
        values: BTreeMap<K, V>,
        senders: Vec<mpsc::UnboundedSender<MapDiff<K, V>>>,
    }

    impl<K: Ord, V> MutableBTreeState<K, V> {
        // TODO should this inline ?
        #[inline]
        fn notify<B: FnMut() -> MapDiff<K, V>>(&mut self, mut change: B) {
            self.senders.retain(|sender| {
                sender.unbounded_send(change()).is_ok()
            });
        }

        // If there is only 1 sender then it won't clone at all.
        // If there is more than 1 sender then it will clone N-1 times.
        // TODO verify that this works correctly
        #[inline]
        fn notify_clone<A, F>(&mut self, value: Option<A>, mut f: F) where A: Clone, F: FnMut(A) -> MapDiff<K, V> {
            if let Some(value) = value {
                let mut len = self.senders.len();

                if len > 0 {
                    let mut copy = Some(value);

                    self.senders.retain(move |sender| {
                        let value = copy.take().unwrap();

                        len -= 1;

                        // This isn't the last element
                        if len != 0 {
                            copy = Some(value.clone());
                        }

                        sender.unbounded_send(f(value)).is_ok()
                    });
                }
            }
        }

        #[inline]
        fn change<A, F>(&self, f: F) -> Option<A> where F: FnOnce() -> A {
            if self.senders.is_empty() {
                None

            } else {
                Some(f())
            }
        }

        fn clear(&mut self) {
            if !self.values.is_empty() {
                self.values.clear();

                self.notify(|| MapDiff::Clear {});
            }
        }
    }

    impl<K: Ord + Clone, V> MutableBTreeState<K, V> {
        fn remove(&mut self, key: &K) -> Option<V> {
            let value = self.values.remove(key)?;

            let key = self.change(|| key.clone());
            self.notify_clone(key, |key| MapDiff::Remove { key });

            Some(value)
        }
    }

    impl<K: Clone + Ord, V: Clone> MutableBTreeState<K, V> {
        fn entries(values: &BTreeMap<K, V>) -> Vec<(K, V)> {
            values.iter().map(|(k, v)| {
                (k.clone(), v.clone())
            }).collect()
        }

        fn replace(&mut self, values: BTreeMap<K, V>) {
            let entries = self.change(|| Self::entries(&values));

            self.values = values;

            self.notify_clone(entries, |entries| MapDiff::Replace { entries });
        }

        fn insert(&mut self, key: K, value: V) -> Option<V> {
            let x = self.change(|| (key.clone(), value.clone()));

            if let Some(value) = self.values.insert(key, value) {
                self.notify_clone(x, |(key, value)| MapDiff::Update { key, value });
                Some(value)

            } else {
                self.notify_clone(x, |(key, value)| MapDiff::Insert { key, value });
                None
            }
        }

        fn signal_map(&mut self) -> MutableSignalMap<K, V> {
            let (sender, receiver) = mpsc::unbounded();

            if !self.values.is_empty() {
                sender.unbounded_send(MapDiff::Replace {
                    entries: Self::entries(&self.values),
                }).unwrap();
            }

            self.senders.push(sender);

            MutableSignalMap {
                receiver,
                store_handle: self.store_handle().clone()
            }
        }
    }

    // impl<K: Clone + Ord, V: Clone> MutableBTreeState<K, V> {
    //     fn entries(values: &BTreeMap<K, V>) -> Vec<(K, V)> {
    //         values.into_iter().map(|(k, v)| {
    //             (k.clone(), v.clone())
    //         }).collect()
    //     }
    // 
    //     fn replace(&mut self, values: BTreeMap<K, V>) {
    //         let entries = self.change(|| Self::entries(&values));
    // 
    //         self.values = values;
    // 
    //         self.notify_clone(entries, |entries| MapDiff::Replace { entries });
    //     }
    // 
    //     fn insert(&mut self, key: K, value: V) -> Option<V> {
    //         if let Some(old_value) = self.values.insert(key, value) {
    //             self.notify(|| MapDiff::Update { key, value });
    //             Some(old_value)
    // 
    //         } else {
    //             self.notify(|| MapDiff::Insert { key, value });
    //             None
    //         }
    //     }
    // 
    //     fn signal_map(&mut self) -> MutableSignalMap<K, V> {
    //         let (sender, receiver) = mpsc::unbounded();
    // 
    //         if !self.values.is_empty() {
    //             sender.unbounded_send(MapDiff::Replace {
    //                 entries: Self::entries(&self.values),
    //             }).unwrap();
    //         }
    // 
    //         self.senders.push(sender);
    // 
    //         MutableSignalMap {
    //             receiver
    //         }
    //     }
    // }


    macro_rules! make_shared {
        ($t:ty) => {
            impl<'a, K, V> PartialEq<BTreeMap<K, V>> for $t where K: PartialEq<K>, V: PartialEq<V> {
                #[inline] fn eq(&self, other: &BTreeMap<K, V>) -> bool { **self == *other }
                #[inline] fn ne(&self, other: &BTreeMap<K, V>) -> bool { **self != *other }
            }

            impl<'a, K, V> PartialEq<$t> for $t where K: PartialEq<K>, V: PartialEq<V> {
                #[inline] fn eq(&self, other: &$t) -> bool { *self == **other }
                #[inline] fn ne(&self, other: &$t) -> bool { *self != **other }
            }

            impl<'a, K, V> Eq for $t where K: Eq, V: Eq {}

            impl<'a, K, V> PartialOrd<BTreeMap<K, V>> for $t where K: PartialOrd<K>, V: PartialOrd<V> {
                #[inline]
                fn partial_cmp(&self, other: &BTreeMap<K, V>) -> Option<Ordering> {
                    PartialOrd::partial_cmp(&**self, &*other)
                }
            }

            impl<'a, K, V> PartialOrd<$t> for $t where K: PartialOrd<K>, V: PartialOrd<V> {
                #[inline]
                fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
                    PartialOrd::partial_cmp(&**self, &**other)
                }
            }

            impl<'a, K, V> Ord for $t where K: Ord, V: Ord {
                #[inline]
                fn cmp(&self, other: &Self) -> Ordering {
                    Ord::cmp(&**self, &**other)
                }
            }

            impl<'a, 'b, K, V> Index<&'b K> for $t where K: Ord {
                type Output = V;

                #[inline]
                fn index(&self, key: &'b K) -> &Self::Output {
                    Index::index(&**self, key)
                }
            }

            impl<'a, K, V> Deref for $t {
                type Target = BTreeMap<K, V>;

                #[inline]
                fn deref(&self) -> &Self::Target {
                    &self.lock.values
                }
            }

            impl<'a, K, V> Hash for $t where K: Hash, V: Hash {
                #[inline]
                fn hash<H>(&self, state: &mut H) where H: Hasher {
                    Hash::hash(&**self, state)
                }
            }
        };
    }


    #[derive(Debug)]
    pub struct MutableBTreeMapLockRef<'a, K, V> where K: 'a, V: 'a {
        lock: RwLockReadGuard<'a, MutableBTreeState<K, V>>,
    }

    make_shared!(MutableBTreeMapLockRef<'a, K, V>);


    #[derive(Debug)]
    pub struct MutableBTreeMapLockMut<'a, K, V> where K: 'a, V: 'a {
        lock: RwLockWriteGuard<'a, MutableBTreeState<K, V>>,
    }

    make_shared!(MutableBTreeMapLockMut<'a, K, V>);

    impl<'a, K, V> MutableBTreeMapLockMut<'a, K, V> where K: Ord {
        #[inline]
        pub fn clear(&mut self) {
            self.lock.clear()
        }
    }

    impl<'a, K, V> MutableBTreeMapLockMut<'a, K, V> where K: Ord + Clone {
        #[inline]
        pub fn remove(&mut self, key: &K) -> Option<V> {
            self.lock.remove(key)
        }
    }

    impl<'a, K, V> MutableBTreeMapLockMut<'a, K, V> where K: Ord + Clone, V: Clone {
        #[inline]
        pub fn replace(&mut self, values: BTreeMap<K, V>) {
            self.lock.replace(values)
        }

        #[inline]
        pub fn insert(&mut self, key: K, value: V) -> Option<V> {
            self.lock.insert(key, value)
        }
    }

    // impl<'a, K, V> MutableBTreeMapLockMut<'a, K, V> where K: Ord + Copy, V: Copy {
    //     #[inline]
    //     pub fn replace(&mut self, values: BTreeMap<K, V>) {
    //         self.lock.replace(values)
    //     }
    // 
    //     #[inline]
    //     pub fn insert(&mut self, key: K, value: V) -> Option<V> {
    //         self.lock.insert(key, value)
    //     }
    // }


    // TODO get rid of the Arc
    // TODO impl some of the same traits as BTreeMap
    pub struct MutableBTreeMap<K, V> {
        store_handle: StoreHandle,
        state: Arc<RwLock<MutableBTreeState<K, V>>>
    }

    impl<K, V> MutableBTreeMap<K, V> {
        // TODO deprecate this and replace with From ?
        // #[inline]
        // pub fn with_values(values: BTreeMap<K, V>) -> Self {
        //     Self::from(values)
        // }

        // TODO return Result ?
        #[inline]
        pub fn lock_ref(&self) -> MutableBTreeMapLockRef<K, V> {
            MutableBTreeMapLockRef {
                lock: self.state.read().unwrap(),
            }
        }

        // TODO return Result ?
        #[inline]
        pub fn lock_mut(&self) -> MutableBTreeMapLockMut<K, V> {
            MutableBTreeMapLockMut {
                lock: self.state.write().unwrap(),
            }
        }
    }

    impl<K, V> MutableBTreeMap<K, V> where K: Ord {
        #[inline]
        pub fn new(store_handle: StoreHandle) -> Self {
            Self::new_with_values(BTreeMap::new(), store_handle)
        }

        #[inline]
        pub fn new_with_values<T>(values: T, store_handle: StoreHandle) -> Self where BTreeMap<K, V>: From<T> {
            let state = Arc::new(RwLock::new(MutableBTreeState {
                values: values.into(),
                senders: vec![],
                store_handle: store_handle.clone()
            }));
            Self {
                store_handle,
                state
            }
        }
    }

    // impl<T, K, V> From<T> for MutableBTreeMap<K, V> where BTreeMap<K, V>: From<T> {
    //     #[inline]
    //     fn from_btree_map(values: T) -> Self {
    //         Self(Arc::new(RwLock::new(MutableBTreeState {
    //             values: values.into(),
    //             senders: vec![],
    //         })))
    //     }
    // }

    // impl<K, V> HasSignalMapCloned<K, V> for MutableBTreeMap<K, V> where K: Ord + Clone, V: Clone  {
    //     #[inline]
    //     fn signal_map_cloned(&self) -> MutableSignalMap<K, V> {
    //         self.state.write().unwrap().signal_map_cloned()
    //     }
    // }

    impl<K, V> Provider for MutableBTreeMap<K, V> {
        type YieldedValue = MapDiff<K, V>;

        fn fut_key(&self) -> Option<SpawnedFutureKey> {
            None
        }

        fn register_effect<F>(&self, f: F) -> Result<SpawnedFutureKey, &'static str> 
            where 
                F: Fn(Self::YieldedValue) + Send + 'static 
        {
            todo!()
        }
    }

    impl<K, V> MutableBTreeMap<K, V> where K: Ord + Clone, V: Clone {
        // #[inline]
        // pub fn signal_map_cloned(&self) -> MutableSignalMap<K, V> {
        //     self.0.write().unwrap().signal_map_cloned()
        // }

        // TODO deprecate and rename to keys_cloned
        // TODO replace with MutableBTreeMapKeysCloned
        #[inline]
        pub fn signal_vec_keys(&self) -> MutableBTreeMapKeys<K, V> {
            MutableBTreeMapKeys {
                signal: self.signal_map(),
                keys: vec![],
            }
        }

        // TODO replace with MutableBTreeMapEntriesCloned
        #[inline]
        pub fn entries_cloned(&self) -> MutableBTreeMapEntries<K, V> {
            MutableBTreeMapEntries {
                signal: self.signal_map(),
                keys: vec![],
            }
        }
    }

    impl<K, V> HasSignalMap<K, V> for MutableBTreeMap<K, V> where K: Ord + Clone, V: Clone  {
        #[inline]
        fn signal_map(&self) -> MutableSignalMap<K, V> {
            self.state.write().unwrap().signal_map()
        }
    }

    impl<K, V> HasStoreHandle for MutableBTreeMap<K, V> {
        fn store_handle(&self) -> &StoreHandle {
            &self.store_handle
        }
    }

    impl<K, V> MutableBTreeMap<K, V> where K: Ord + Copy, V: Copy {
        // #[inline]
        // pub fn signal_map(&self) -> MutableSignalMap<K, V> {
        //     self.0.write().unwrap().signal_map()
        // }

        // TODO deprecate and rename to entries
        #[inline]
        pub fn signal_vec_entries(&self) -> MutableBTreeMapEntries<K, V> {
            MutableBTreeMapEntries {
                signal: self.signal_map(),
                keys: vec![],
            }
        }
    }

    impl<K, V> fmt::Debug for MutableBTreeMap<K, V> where K: fmt::Debug, V: fmt::Debug {
        fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
            let state = self.state.read().unwrap();

            fmt.debug_tuple("MutableBTreeMap")
                .field(&state.values)
                .finish()
        }
    }

    #[cfg(feature = "serde")]
    impl<K, V> serde::Serialize for MutableBTreeMap<K, V> where BTreeMap<K, V>: serde::Serialize {
        #[inline]
        fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error> where S: serde::Serializer {
            self.state.read().unwrap().values.serialize(serializer)
        }
    }

    // #[cfg(feature = "serde")]
    // impl<'de, K, V> serde::Deserialize<'de> for MutableBTreeMap<K, V> where BTreeMap<K, V>: serde::Deserialize<'de> {
    //     #[inline]
    //     fn deserialize<D>(deserializer: D) -> Result<Self, D::Error> where D: serde::Deserializer<'de> {
    //         <BTreeMap<K, V>>::deserialize(deserializer).map(MutableBTreeMap::with_values)
    //     }
    // }

    // impl<K, V> Default for MutableBTreeMap<K, V> where K: Ord {
    //     #[inline]
    //     fn default() -> Self {
    //         MutableBTreeMap::new()
    //     }
    // }

    impl<K, V> Clone for MutableBTreeMap<K, V> {
        #[inline]
        fn clone(&self) -> Self {
            Self {
                store_handle: self.store_handle.clone(),
                state: self.state.clone()
            }
        }
    }

    #[derive(Debug)]
    #[has_store_handle_macro::has_store_handle]
    #[must_use = "SignalMaps do nothing unless polled"]
    pub struct MutableSignalMap<K, V> {
        receiver: mpsc::UnboundedReceiver<MapDiff<K, V>>,
    }

    impl<K, V> Unpin for MutableSignalMap<K, V> {}

    impl<K, V> SignalMap for MutableSignalMap<K, V> {
        type Key = K;
        type Value = V;

        #[inline]
        fn poll_map_change(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<MapDiff<Self::Key, Self::Value>>> {
            self.receiver.poll_next_unpin(cx)
        }
    }


    #[derive(Debug)]
    #[must_use = "SignalVecs do nothing unless polled"]
    pub struct MutableBTreeMapKeys<K, V> {
        signal: MutableSignalMap<K, V>,
        keys: Vec<K>,
    }

    impl<K, V> Unpin for MutableBTreeMapKeys<K, V> {}

    impl<K, V> SignalVec for MutableBTreeMapKeys<K, V> where K: Ord + Clone {
        type Item = K;

        #[inline]
        fn poll_vec_change(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<VecDiff<Self::Item>>> {
            loop {
                return match self.signal.poll_map_change_unpin(cx) {
                    Poll::Ready(Some(diff)) => match diff {
                        MapDiff::Replace { entries } => {
                            // TODO verify that it is in sorted order ?
                            self.keys = entries.into_iter().map(|(k, _)| k).collect();
                            Poll::Ready(Some(VecDiff::Replace { values: self.keys.clone() }))
                        },
                        MapDiff::Insert { key, value: _ } => {
                            let index = self.keys.binary_search(&key).unwrap_err();
                            self.keys.insert(index, key.clone());
                            Poll::Ready(Some(VecDiff::InsertAt { index, value: key }))
                        },
                        MapDiff::Update { .. } => {
                            continue;
                        },
                        MapDiff::Remove { key } => {
                            let index = self.keys.binary_search(&key).unwrap();
                            self.keys.remove(index);
                            Poll::Ready(Some(VecDiff::RemoveAt { index }))
                        },
                        MapDiff::Clear {} => {
                            self.keys.clear();
                            Poll::Ready(Some(VecDiff::Clear {}))
                        },
                    },
                    Poll::Ready(None) => Poll::Ready(None),
                    Poll::Pending => Poll::Pending,
                };
            }
        }
    }


    #[derive(Debug)]
    #[must_use = "SignalVecs do nothing unless polled"]
    pub struct MutableBTreeMapEntries<K, V> {
        signal: MutableSignalMap<K, V>,
        keys: Vec<K>,
    }

    impl<K, V> Unpin for MutableBTreeMapEntries<K, V> {}

    impl<K, V> SignalVec for MutableBTreeMapEntries<K, V> where K: Ord + Clone {
        type Item = (K, V);

        #[inline]
        fn poll_vec_change(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<VecDiff<Self::Item>>> {
            self.signal.poll_map_change_unpin(cx).map(|opt| opt.map(|diff| {
                match diff {
                    MapDiff::Replace { entries } => {
                        // TODO verify that it is in sorted order ?
                        self.keys = entries.iter().map(|(k, _)| k.clone()).collect();
                        VecDiff::Replace { values: entries }
                    },
                    MapDiff::Insert { key, value } => {
                        let index = self.keys.binary_search(&key).unwrap_err();
                        self.keys.insert(index, key.clone());
                        VecDiff::InsertAt { index, value: (key, value) }
                    },
                    MapDiff::Update { key, value } => {
                        let index = self.keys.binary_search(&key).unwrap();
                        VecDiff::UpdateAt { index, value: (key, value) }
                    },
                    MapDiff::Remove { key } => {
                        let index = self.keys.binary_search(&key).unwrap();
                        self.keys.remove(index);
                        VecDiff::RemoveAt { index }
                    },
                    MapDiff::Clear {} => {
                        self.keys.clear();
                        VecDiff::Clear {}
                    },
                }
            }))
        }
    }

    pub mod observable_btree_map {
        use std::future::Future;

        use crate::signal_map::{MapDiff, MutableBTreeMapLockMut, MutableSignalMap};
        use crate::store::{SpawnedFutureKey, StoreAccess, StoreHandle};
        use crate::traits::{HasSignalMap, HasStoreHandle, IsObservable, ObserveMap, Provider, SSS};

        use super::{MutableBTreeMap, MutableBTreeMapLockRef, SignalMapExt};

        trait LockMut<K, V> {
            fn lock_mut(&self) -> MutableBTreeMapLockMut<'_, K, V>;
        }

        #[derive(Debug)]
        pub struct ObservableBTreeMap<K, V> {
            pub(crate) store_handle: StoreHandle,
            pub(crate) map: MutableBTreeMap<K, V>,
            pub(crate) fut_key: SpawnedFutureKey
        }

        impl<K, V> HasStoreHandle for ObservableBTreeMap<K, V> {
            fn store_handle(&self) -> &StoreHandle {
                &self.store_handle
            }
        }
        
        impl<K: Clone + Ord + SSS, V: Clone + SSS> Provider for ObservableBTreeMap<K, V> {
            type YieldedValue = MapDiff<K, V>;
            
            fn fut_key(&self) -> Option<SpawnedFutureKey> {
                Some(self.fut_key)
            }

            fn register_effect<F>(&self, f: F) -> Result<SpawnedFutureKey, &'static str>
                where
                    F: Fn(Self::YieldedValue) + Send + 'static
            {
                let fut = self.signal_map().for_each(move |v| {
                    f(v);
                    async {}
                });

                let mut lock = self.store_handle().clone();
                let key = lock.spawn_fut(self.fut_key(), fut);
                Ok(key)
            }
        }

        impl<K: Ord + Clone, V: Clone> HasSignalMap<K, V> for ObservableBTreeMap<K, V> {
            fn signal_map(&self) -> MutableSignalMap<K, V> {
                self.map.signal_map()
            }
        }

        // impl<K, V> HasStoreHandle for ObservableBTreeMap<K, V> {
        //     fn store_handle(&self) -> &StoreHandle {
        //         &self.store_handle
        //     }
        // }

        impl<K, V> Clone for ObservableBTreeMap<K, V> {
            fn clone(&self) -> Self {
                Self {
                    store_handle: self.store_handle.clone(),
                    map: self.map.clone(),
                    fut_key: self.fut_key
                }
            }
        }

        impl<K: Ord, V> IsObservable for ObservableBTreeMap<K, V>  {
            type Inner = MutableBTreeMap<K, V>;
            fn new_inner(store_handle: StoreHandle) -> Self::Inner {
                MutableBTreeMap::new(store_handle)
            }

            fn new(store_handle: StoreHandle, inner: Self::Inner, fut_key: SpawnedFutureKey) -> Self {
                Self {
                    store_handle,
                    map: inner,
                    fut_key,
                }
            }
        }

        impl<K, V> ObservableBTreeMap<K, V> {

            // TODO return Result ?
            #[inline]
            pub fn lock_ref(&self) -> MutableBTreeMapLockRef<K, V> {
                self.map.lock_ref()
            }
        }
        
        impl<T, O, K1, K2, V1, V2, F, U> ObserveMap<T, O, K1, V1, F, U> for T
            where
                T: HasSignalMap<K1, V1> + Provider,
                O: IsObservable<Inner = MutableBTreeMap<K2, V2>>,
                <O as IsObservable>::Inner: Clone,
                K1: Clone + Ord,
                V1: Clone,
                F: Fn(MutableSignalMap<K1, V1>, <O as IsObservable>::Inner) -> U,
                U: Future<Output = ()>  + Send + 'static
        {
            fn observe_map(&self, f: F) -> O {
                let mut store_handle = self.store_handle().clone();
                let out = O::new_inner(store_handle.clone());
                let out_clone = out.clone();
                let fut = f(self.signal_map(), out_clone);

                let fut_key = store_handle.spawn_fut(self.fut_key(), fut);
                O::new(store_handle, out, fut_key)
            }
        }

        #[cfg(test)]
        mod tests {
            use std::thread;
            use std::time::Duration;

            use crate::signal_map::{MapDiff, MutableBTreeMap};
            use crate::signal_map::observable_btree_map::{ObservableBTreeMap, ObserveMap};
            use crate::signal_map::SignalMapExt;
            use crate::store::{Manager, RxStore};

            fn handle_map_diff(diff: MapDiff<i32, i32>, output: MutableBTreeMap<i32, i32>)
            {
                match diff {
                    MapDiff::Replace { entries } => {
                        let mut lock = output.lock_mut();
                        entries.iter().for_each(|(k, v)| {
                            lock.insert(*k, *v * 3);
                        })
                    },
                    MapDiff::Insert { key, value } |
                    MapDiff::Update { key, value } => {
                        let mut lock = output.lock_mut();
                        lock.insert(key, value * 3);
                    },
                    MapDiff::Remove { key } => {
                        let mut lock = output.lock_mut();
                        //let k = *lock.iter().find(|(_, v)| *v == &key).unwrap().0;
                        lock.remove(&key);
                    },
                    MapDiff::Clear {} => output.lock_mut().clear()
                }
            }
            
            #[test]
            fn it_observes_map_cloned() {
                let store = RxStore::new();
                let map = store.new_mutable_btree_map();
                {
                    let mut lock = map.lock_mut();
                    lock.insert("one".to_owned(), 1);
                    lock.insert("two".to_owned(), 2);
                }
                let obsv_map: ObservableBTreeMap<i32, String> = map.observe_map(|input, output: MutableBTreeMap<i32, String>| {
                    input.for_each(move |diff| {
                        match diff {
                            MapDiff::Replace { entries } => {
                                let mut lock = output.lock_mut();
                                entries.iter().for_each(|(k, v)| {
                                    lock.insert(*v, k.clone());
                                })
                            }
                            MapDiff::Insert { key, value } |
                            MapDiff::Update { key, value } => {
                                let mut lock = output.lock_mut();
                                lock.insert(value, key);
                            }
                            MapDiff::Remove { key } => {
                                let mut lock = output.lock_mut();
                                let k = *lock.iter().find(|(_, v)| *v == &key).unwrap().0;
                                lock.remove(&k);
                            }
                            MapDiff::Clear {} => output.lock_mut().clear()
                        }
                        async {}
                    })
                });

                {
                    let mut lock = map.lock_mut();
                    lock.insert("three".to_owned(), 3);
                    lock.insert("four".to_owned(), 4);
                }
                thread::sleep(Duration::from_millis(500));

                assert_eq!(obsv_map.lock_ref().get(&4).unwrap(), &"four".to_string());
            }

            #[test]
            fn it_clears_observable_map() {
                let store = RxStore::new();
                let map = store.new_mutable_btree_map();
                {
                    let mut lock = map.lock_mut();
                    lock.insert(1, 1);
                    lock.insert(2, 2);
                }
                let obsv_map: ObservableBTreeMap<i32, i32> = map.observe_map(
                    |input, output: MutableBTreeMap<i32, i32>| {
                        input.for_each(move |diff| {
                            handle_map_diff(diff, output.clone());
                            async {}
                        })
                    }
                );

                {
                    let mut lock = map.lock_mut();
                    lock.clear();
                }
                thread::sleep(Duration::from_millis(500));

                assert!(obsv_map.lock_ref().is_empty());
            }
            
            #[test]
            fn it_removes_from_observable_map() {
                let store = RxStore::new();
                let map = store.new_mutable_btree_map();
                {
                    let mut lock = map.lock_mut();
                    lock.insert(1, 1);
                    lock.insert(2, 2);
                }
                let obsv_map: ObservableBTreeMap<i32, i32> = map.observe_map(
                    |input, output: MutableBTreeMap<i32, i32>| {
                        input.for_each(move |diff| {
                            handle_map_diff(diff, output.clone());
                            async {}
                        })
                    }
                );

                {
                    let mut lock = map.lock_mut();
                    lock.remove(&1);
                }
                thread::sleep(Duration::from_millis(500));

                assert!(obsv_map.lock_ref().get(&1).is_none());
            }

            #[test]
            fn it_observes_observable_map() {
                let store = RxStore::new();
                let map = store.new_mutable_btree_map();
                {
                    let mut lock = map.lock_mut();
                    lock.insert(1, 1);
                    lock.insert(2, 2);
                }
                let obsv_map_one: ObservableBTreeMap<i32, i32> = map.observe_map(
                    |input, output: MutableBTreeMap<i32, i32>| {
                        input.for_each(move |diff| {
                            handle_map_diff(diff, output.clone());
                            async {}
                        })
                    }
                );

                thread::sleep(Duration::from_millis(500));

                //let obsv_map_one_clone = obsv_map_one.clone();
                let obsv_map_two: ObservableBTreeMap<i32, i32> = obsv_map_one.observe_map(
                    |input, output: MutableBTreeMap<i32, i32>| {
                        input.for_each(move |diff| {
                            handle_map_diff(diff, output.clone());
                            async {}
                        })
                    }
                );

                {
                    let mut lock = map.lock_mut();
                    lock.insert(3, 3);
                    lock.insert(4, 4);
                }

                thread::sleep(Duration::from_millis(500));

                // obsv_map_two.lock_ref().iter().for_each(|(k, v)| {
                //     println!("k: {}, v: {}", k, v);
                // });
                // 
                // println!("{:?}", obsv_map_one);
                // println!("{:?}", obsv_map_two);

                assert_eq!(obsv_map_two.lock_ref().get(&2).unwrap(), &18);
            }
        }

        

    }
}