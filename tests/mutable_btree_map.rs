use std::collections::BTreeMap;
use std::task::Poll;
use rx_store::signal_map::{MapDiff, MutableBTreeMap, MutableBTreeMapLockMut};
use rx_store::store::{Manager, Store};
use rx_store::traits::{HasSignalMap, HasSignalMapCloned};

mod util;

#[derive(Clone, Debug, PartialEq, Eq)]
struct TestValueType {
    inner: i32,
}

fn emits_diffs<K, V, F>(f: F, polls: Vec<Poll<Option<MapDiff<K, V>>>>)
    where F: FnOnce(&mut MutableBTreeMapLockMut<K, V>),
          K: Ord + Copy + std::fmt::Debug,
          V: PartialEq + Copy + std::fmt::Debug 
{
    let store = Store::new();
    let map = store.new_mutable_btree_map::<K, V>();
    //let map = MutableBTreeMap::<K, V>::new();
    assert_eq!(util::get_signal_map_polls(map.signal_map(), || {
        {
            let mut v = map.lock_mut();
            f(&mut v);
        }
        drop(map);
    }), polls);
}

fn emits_diffs_cloned<K, V, F>(f: F, polls: Vec<Poll<Option<MapDiff<K, V>>>>)
    where F: FnOnce(&mut MutableBTreeMapLockMut<K, V>),
          K: Ord + Clone + std::fmt::Debug,
          V: PartialEq + Clone + std::fmt::Debug 
{
    let store = Store::new();
    let map = store.new_mutable_btree_map::<K, V>();
    //let map = MutableBTreeMap::<K, V>::new();
    assert_eq!(util::get_signal_map_polls(map.signal_map_cloned(), || {
        {
            let mut v = map.lock_mut();
            f(&mut v);
        }
        drop(map);
    }), polls);
}

// fn emits_diffs_cloned<K, V, F>(f: F, polls: Vec<Poll<Option<MapDiff<K, V>>>>)
//     where F: FnOnce(&mut MutableBTreeMapLockMut<K, V>),
//           K: Ord + Clone + std::fmt::Debug,
//           V: PartialEq + Clone + std::fmt::Debug {
// 
//     let map = MutableBTreeMap::<K, V>::new();
//     assert_eq!(util::get_signal_map_polls(map.signal_map_cloned(), || {
//         {
//             let mut v = map.lock_mut();
//             f(&mut v);
//         }
//         drop(map);
//     }), polls);
// }

#[test]
fn insert_and_remove() {
    let store = Store::new();
    let m = store.new_mutable_btree_map::<u8, i8>();
    //let m = MutableBTreeMap::<u8, i8>::new();
    let mut writer = m.lock_mut();
    writer.insert(8, -8);
    assert_eq!(writer.get(&8).unwrap(), &-8);

    writer.insert(8, 100);
    assert_eq!(writer.get(&8).unwrap(), &100);

    writer.remove(&8);
    assert_eq!(writer.get(&8), None);
}

#[test]
fn clear() {
    let store = Store::new();
    let m = store.new_mutable_btree_map::<u8, i8>();
    //let m = MutableBTreeMap::<u8, i8>::new();
    let mut writer = m.lock_mut();
    writer.insert(80, -80);
    assert_eq!(writer.get(&80).unwrap(), &-80);

    writer.clear();
    assert_eq!(writer.get(&8), None);
}

#[test]
fn insert_cloned() {
    let store = Store::new();
    let m = store.new_mutable_btree_map::<&'static str, TestValueType>();
    //let m = MutableBTreeMap::<&'static str, TestValueType>::new();
    let mut writer = m.lock_mut();
    writer.insert_cloned("test", TestValueType {inner: 294});
    assert_eq!(writer.get(&"test").unwrap(), &TestValueType {inner: 294});
}

#[test]
fn signal_map() {
    emits_diffs(|writer| {
        writer.insert(1, 1);
        writer.remove(&1);
    }, vec![
        Poll::Pending,
        Poll::Ready(Some(MapDiff::Insert { key: 1, value: 1 })),
        Poll::Ready(Some(MapDiff::Remove { key: 1 })),
        Poll::Ready(None)
    ]);
}

#[test]
fn signal_map_cloned() {
    emits_diffs_cloned(|writer| {
        writer.insert_cloned(1, TestValueType {inner: 42});
        writer.remove(&1);
    }, vec![
        Poll::Pending,
        Poll::Ready(Some(MapDiff::Insert { key: 1, value: TestValueType {inner: 42} })),
        Poll::Ready(Some(MapDiff::Remove { key: 1 })),
        Poll::Ready(None)
    ]);
}

#[test]
fn is_from_btreemap() {
    let store = Store::new();
    let src = BTreeMap::from([(1,"test".to_string())]);
    let _out: MutableBTreeMap<u8, String> = store.new_mutable_btree_map_w_values(src);
    
    //let _out: MutableBTreeMap<u8, String> = MutableBTreeMap::from(src);

    // let src = BTreeMap::from([(1,"test".to_string())]);
    // let _out: MutableBTreeMap<u8, String> = src.into();
    // 
    // let _out: MutableBTreeMap<u8, String> = [(1,"foo".to_string())].into();
}