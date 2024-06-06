use std::cmp::{Ordering, PartialOrd};
use std::task::Poll;

use rx_store::signal_vec::{MutableVecLockMut, VecDiff};
use rx_store::store::{Manager, RxStore};

mod util;


fn is_eq<F>(input: Vec<u32>, output: Vec<u32>, f: F, polls: Vec<Poll<Option<VecDiff<u32>>>>)
    where F: FnOnce(&mut MutableVecLockMut<u32>) 
{
    let store = RxStore::new();
    let v = store.new_mutable_vec_w_values(input);

    let mut end = None;

    assert_eq!(util::get_signal_vec_polls(v.signal_vec(), || {
        {
            let mut v = v.lock_mut();
            f(&mut v);
            end = Some(v.to_vec());
        }
        drop(v);
    }), polls);

    assert_eq!(end.unwrap(), output);
}


#[test]
fn test_push() {
    is_eq(vec![], vec![5, 10, 2], |v| {
        v.push(5);
        v.push(10);
        v.push(2);
    }, vec![
        Poll::Pending,
        Poll::Ready(Some(VecDiff::Push { value: 5 })),
        Poll::Ready(Some(VecDiff::Push { value: 10 })),
        Poll::Ready(Some(VecDiff::Push { value: 2 })),
        Poll::Ready(None),
    ]);
}


#[test]
fn test_move_from_to() {
    is_eq(vec![5, 10, 15], vec![5, 10, 15], |v| v.move_from_to(0, 0), vec![
        Poll::Ready(Some(VecDiff::Replace { values: vec![5, 10, 15] } )),
        Poll::Pending,
        Poll::Ready(None),
    ]);

    is_eq(vec![5, 10, 15], vec![10, 5, 15], |v| v.move_from_to(0, 1), vec![
        Poll::Ready(Some(VecDiff::Replace { values: vec![5, 10, 15] } )),
        Poll::Pending,
        Poll::Ready(Some(VecDiff::Move { old_index: 0, new_index: 1 } )),
        Poll::Ready(None),
    ]);

    is_eq(vec![5, 10, 15], vec![10, 5, 15], |v| v.move_from_to(1, 0), vec![
        Poll::Ready(Some(VecDiff::Replace { values: vec![5, 10, 15] } )),
        Poll::Pending,
        Poll::Ready(Some(VecDiff::Move { old_index: 1, new_index: 0 } )),
        Poll::Ready(None),
    ]);

    is_eq(vec![5, 10, 15], vec![10, 15, 5], |v| v.move_from_to(0, 2), vec![
        Poll::Ready(Some(VecDiff::Replace { values: vec![5, 10, 15] } )),
        Poll::Pending,
        Poll::Ready(Some(VecDiff::Move { old_index: 0, new_index: 2 } )),
        Poll::Ready(None),
    ]);
}


#[test]
fn test_swap() {
    is_eq(vec![5, 10], vec![10, 5], |v| v.swap(0, 1), vec![
        Poll::Ready(Some(VecDiff::Replace { values: vec![5, 10] } )),
        Poll::Pending,
        Poll::Ready(Some(VecDiff::Move { old_index: 0, new_index: 1 } )),
        Poll::Ready(None),
    ]);

    is_eq(vec![5, 10, 15], vec![10, 5, 15], |v| v.swap(0, 1), vec![
        Poll::Ready(Some(VecDiff::Replace { values: vec![5, 10, 15] } )),
        Poll::Pending,
        Poll::Ready(Some(VecDiff::Move { old_index: 0, new_index: 1 } )),
        Poll::Ready(None),
    ]);

    is_eq(vec![5, 10, 15], vec![15, 10, 5], |v| v.swap(0, 2), vec![
        Poll::Ready(Some(VecDiff::Replace { values: vec![5, 10, 15] } )),
        Poll::Pending,
        Poll::Ready(Some(VecDiff::Move { old_index: 0, new_index: 2 } )),
        Poll::Ready(Some(VecDiff::Move { old_index: 1, new_index: 0 } )),
        Poll::Ready(None),
    ]);

    is_eq(vec![5, 10, 15], vec![5, 15, 10], |v| v.swap(1, 2), vec![
        Poll::Ready(Some(VecDiff::Replace { values: vec![5, 10, 15] } )),
        Poll::Pending,
        Poll::Ready(Some(VecDiff::Move { old_index: 1, new_index: 2 } )),
        Poll::Ready(None),
    ]);

    is_eq(vec![5, 10, 15], vec![5, 15, 10], |v| v.swap(2, 1), vec![
        Poll::Ready(Some(VecDiff::Replace { values: vec![5, 10, 15] } )),
        Poll::Pending,
        Poll::Ready(Some(VecDiff::Move { old_index: 2, new_index: 1 } )),
        Poll::Ready(None),
    ]);

    is_eq(vec![5, 10, 15], vec![15, 10, 5], |v| v.swap(2, 0), vec![
        Poll::Ready(Some(VecDiff::Replace { values: vec![5, 10, 15] } )),
        Poll::Pending,
        Poll::Ready(Some(VecDiff::Move { old_index: 2, new_index: 0 } )),
        Poll::Ready(Some(VecDiff::Move { old_index: 1, new_index: 2 } )),
        Poll::Ready(None),
    ]);
}


#[test]
fn test_reverse() {
    is_eq(vec![], vec![], |v| v.reverse(), vec![
        Poll::Pending,
        Poll::Ready(None),
    ]);

    is_eq(vec![1], vec![1], |v| v.reverse(), vec![
        Poll::Ready(Some(VecDiff::Replace { values: vec![1] } )),
        Poll::Pending,
        Poll::Ready(None),
    ]);

    is_eq(vec![1, 2], vec![2, 1], |v| v.reverse(), vec![
        Poll::Ready(Some(VecDiff::Replace { values: vec![1, 2] } )),
        Poll::Pending,
        Poll::Ready(Some(VecDiff::Move { old_index: 1, new_index: 0 } )),
        Poll::Ready(None),
    ]);

    is_eq(vec![1, 2, 3], vec![3, 2, 1], |v| v.reverse(), vec![
        Poll::Ready(Some(VecDiff::Replace { values: vec![1, 2, 3] } )),
        Poll::Pending,
        Poll::Ready(Some(VecDiff::Move { old_index: 2, new_index: 0 } )),
        Poll::Ready(Some(VecDiff::Move { old_index: 2, new_index: 1 } )),
        Poll::Ready(None),
    ]);

    is_eq(vec![1, 2, 3, 4], vec![4, 3, 2, 1], |v| v.reverse(), vec![
        Poll::Ready(Some(VecDiff::Replace { values: vec![1, 2, 3, 4] } )),
        Poll::Pending,
        Poll::Ready(Some(VecDiff::Move { old_index: 3, new_index: 0 } )),
        Poll::Ready(Some(VecDiff::Move { old_index: 3, new_index: 1 } )),
        Poll::Ready(Some(VecDiff::Move { old_index: 3, new_index: 2 } )),
        Poll::Ready(None),
    ]);

    is_eq(vec![1, 2, 3, 4, 5], vec![5, 4, 3, 2, 1], |v| v.reverse(), vec![
        Poll::Ready(Some(VecDiff::Replace { values: vec![1, 2, 3, 4, 5] } )),
        Poll::Pending,
        Poll::Ready(Some(VecDiff::Move { old_index: 4, new_index: 0 } )),
        Poll::Ready(Some(VecDiff::Move { old_index: 4, new_index: 1 } )),
        Poll::Ready(Some(VecDiff::Move { old_index: 4, new_index: 2 } )),
        Poll::Ready(Some(VecDiff::Move { old_index: 4, new_index: 3 } )),
        Poll::Ready(None),
    ]);

    is_eq(vec![1, 2, 3, 4, 5, 6], vec![6, 5, 4, 3, 2, 1], |v| v.reverse(), vec![
        Poll::Ready(Some(VecDiff::Replace { values: vec![1, 2, 3, 4, 5, 6] } )),
        Poll::Pending,
        Poll::Ready(Some(VecDiff::Move { old_index: 5, new_index: 0 } )),
        Poll::Ready(Some(VecDiff::Move { old_index: 5, new_index: 1 } )),
        Poll::Ready(Some(VecDiff::Move { old_index: 5, new_index: 2 } )),
        Poll::Ready(Some(VecDiff::Move { old_index: 5, new_index: 3 } )),
        Poll::Ready(Some(VecDiff::Move { old_index: 5, new_index: 4 } )),
        Poll::Ready(None),
    ]);
}


#[test]
fn test_eq() {
    let store = RxStore::new();
    let a = store.new_mutable_vec_w_values(vec![1, 2, 3, 4, 5]);
    let b = store.new_mutable_vec_w_values(vec![1, 2, 3, 4, 5]);

    assert_eq!(a.lock_ref(), b.lock_ref());
    assert_eq!(*a.lock_ref(), *b.lock_ref());
    assert_eq!(a.lock_ref(), vec![1, 2, 3, 4, 5].as_slice());
}


#[test]
fn test_ord() {
    let store = RxStore::new();
    let a = store.new_mutable_vec_w_values(vec![1, 2, 3, 4, 5]);
    let b = store.new_mutable_vec_w_values(vec![1, 2, 3, 4, 5]);

    let b = b.lock_ref();

    {
        assert_eq!(a.lock_ref().partial_cmp(&b), Some(Ordering::Equal));
        assert_eq!(a.lock_ref().cmp(&b), Ordering::Equal);
    }
}


#[test]
fn test_drain() {
    is_eq(vec![], vec![], |v| drop(v.drain(..)), vec![
        Poll::Pending,
        Poll::Ready(None),
    ]);

    is_eq(vec![5, 10, 15], vec![5, 10, 15], |v| drop(v.drain(0..0)), vec![
        Poll::Ready(Some(VecDiff::Replace { values: vec![5, 10, 15] } )),
        Poll::Pending,
        Poll::Ready(None),
    ]);

    is_eq(vec![5, 10, 15], vec![5, 10, 15], |v| drop(v.drain(1..1)), vec![
        Poll::Ready(Some(VecDiff::Replace { values: vec![5, 10, 15] } )),
        Poll::Pending,
        Poll::Ready(None),
    ]);

    is_eq(vec![5, 10, 15], vec![5, 10, 15], |v| drop(v.drain(2..2)), vec![
        Poll::Ready(Some(VecDiff::Replace { values: vec![5, 10, 15] } )),
        Poll::Pending,
        Poll::Ready(None),
    ]);

    is_eq(vec![5, 10, 15], vec![], |v| drop(v.drain(..)), vec![
        Poll::Ready(Some(VecDiff::Replace { values: vec![5, 10, 15] } )),
        Poll::Pending,
        Poll::Ready(Some(VecDiff::Clear {})),
        Poll::Ready(None),
    ]);

    is_eq(vec![5, 10, 15], vec![], |v| drop(v.drain(0..)), vec![
        Poll::Ready(Some(VecDiff::Replace { values: vec![5, 10, 15] } )),
        Poll::Pending,
        Poll::Ready(Some(VecDiff::Clear {})),
        Poll::Ready(None),
    ]);

    is_eq(vec![5, 10, 15], vec![5, 10], |v| drop(v.drain(2..)), vec![
        Poll::Ready(Some(VecDiff::Replace { values: vec![5, 10, 15] } )),
        Poll::Pending,
        Poll::Ready(Some(VecDiff::Pop {})),
        Poll::Ready(None),
    ]);

    is_eq(vec![5, 10, 15], vec![5], |v| drop(v.drain(1..)), vec![
        Poll::Ready(Some(VecDiff::Replace { values: vec![5, 10, 15] } )),
        Poll::Pending,
        Poll::Ready(Some(VecDiff::Pop {})),
        Poll::Ready(Some(VecDiff::Pop {})),
        Poll::Ready(None),
    ]);

    is_eq(vec![5, 10, 15], vec![15], |v| drop(v.drain(0..2)), vec![
        Poll::Ready(Some(VecDiff::Replace { values: vec![5, 10, 15] } )),
        Poll::Pending,
        Poll::Ready(Some(VecDiff::RemoveAt { index: 1 })),
        Poll::Ready(Some(VecDiff::RemoveAt { index: 0 })),
        Poll::Ready(None),
    ]);

    is_eq(vec![5, 10, 15], vec![5, 15], |v| drop(v.drain(1..2)), vec![
        Poll::Ready(Some(VecDiff::Replace { values: vec![5, 10, 15] } )),
        Poll::Pending,
        Poll::Ready(Some(VecDiff::RemoveAt { index: 1 })),
        Poll::Ready(None),
    ]);
}

#[test]
#[should_panic(expected = "slice index starts at 1 but ends at 0")]
fn test_drain_panic_start() {
    let store = RxStore::new();
    store.new_mutable_vec_w_values::<usize>(vec![]).lock_mut().drain(1..);
    //MutableVec::<usize>::new_with_values(vec![]).lock_mut().drain(1..);
}

#[test]
#[should_panic(expected = "range end index 2 out of range for slice of length 1")]
fn test_drain_panic_end() {
    let store = RxStore::new();
    //MutableVec::<usize>::new_with_values(vec![5]).lock_mut().drain(0..2);
    store.new_mutable_vec_w_values::<usize>(vec![5]).lock_mut().drain(0..2);
}

#[test]
#[should_panic(expected = "slice index starts at 1 but ends at 0")]
fn test_drain_panic_swap() {
    let store = RxStore::new();
    store.new_mutable_vec_w_values::<usize>(vec![5]).lock_mut().drain(1..0);
    //MutableVec::<usize>::new_with_values(vec![5]).lock_mut().drain(1..0);
}

#[test]
#[should_panic(expected = "attempted to index slice up to maximum usize")]
fn test_drain_panic_included_end() {
    let store = RxStore::new();
    store.new_mutable_vec_w_values::<usize>(vec![]).lock_mut().drain(..=usize::MAX);
    //MutableVec::<usize>::new_with_values(vec![]).lock_mut().drain(..=usize::MAX);
}


#[test]
fn test_truncate() {
    is_eq(vec![], vec![], |v| v.truncate(0), vec![
        Poll::Pending,
        Poll::Ready(None),
    ]);

    is_eq(vec![], vec![], |v| v.truncate(10), vec![
        Poll::Pending,
        Poll::Ready(None),
    ]);

    is_eq(vec![5, 10, 15], vec![5, 10, 15], |v| v.truncate(3), vec![
        Poll::Ready(Some(VecDiff::Replace { values: vec![5, 10, 15] } )),
        Poll::Pending,
        Poll::Ready(None),
    ]);

    is_eq(vec![5, 10, 15], vec![5, 10, 15], |v| v.truncate(10), vec![
        Poll::Ready(Some(VecDiff::Replace { values: vec![5, 10, 15] } )),
        Poll::Pending,
        Poll::Ready(None),
    ]);

    is_eq(vec![5, 10, 15], vec![5, 10], |v| v.truncate(2), vec![
        Poll::Ready(Some(VecDiff::Replace { values: vec![5, 10, 15] } )),
        Poll::Pending,
        Poll::Ready(Some(VecDiff::Pop {})),
        Poll::Ready(None),
    ]);

    is_eq(vec![5, 10, 15], vec![5], |v| v.truncate(1), vec![
        Poll::Ready(Some(VecDiff::Replace { values: vec![5, 10, 15] } )),
        Poll::Pending,
        Poll::Ready(Some(VecDiff::Pop {})),
        Poll::Ready(Some(VecDiff::Pop {})),
        Poll::Ready(None),
    ]);

    is_eq(vec![5, 10, 15], vec![], |v| v.truncate(0), vec![
        Poll::Ready(Some(VecDiff::Replace { values: vec![5, 10, 15] } )),
        Poll::Pending,
        Poll::Ready(Some(VecDiff::Clear {})),
        Poll::Ready(None),
    ]);
}


#[test]
fn test_extend() {
    is_eq(vec![], vec![], |v| v.extend(0..0), vec![
        Poll::Pending,
        Poll::Ready(None),
    ]);

    is_eq(vec![], vec![0, 1, 2], |v| v.extend(0..3), vec![
        Poll::Pending,
        Poll::Ready(Some(VecDiff::Push { value: 0 })),
        Poll::Ready(Some(VecDiff::Push { value: 1 })),
        Poll::Ready(Some(VecDiff::Push { value: 2 })),
        Poll::Ready(None),
    ]);

    is_eq(vec![0, 1, 2], vec![0, 1, 2, 3, 4, 5], |v| v.extend(3..6), vec![
        Poll::Ready(Some(VecDiff::Replace { values: vec![0, 1, 2] } )),
        Poll::Pending,
        Poll::Ready(Some(VecDiff::Push { value: 3 })),
        Poll::Ready(Some(VecDiff::Push { value: 4 })),
        Poll::Ready(Some(VecDiff::Push { value: 5 })),
        Poll::Ready(None),
    ]);

    is_eq(vec![0, 1, 2], vec![0, 1, 2], |v| v.extend(0..0), vec![
        Poll::Ready(Some(VecDiff::Replace { values: vec![0, 1, 2] } )),
        Poll::Pending,
        Poll::Ready(None),
    ]);
}


#[test]
fn test_apply_vec_diff() {
    is_eq(vec![], vec![11, 10, 0, 3], |v| {
        MutableVecLockMut::apply_vec_diff(v, VecDiff::Push { value: 6 });
        MutableVecLockMut::apply_vec_diff(v, VecDiff::Pop {});
        MutableVecLockMut::apply_vec_diff(v, VecDiff::InsertAt { index: 0, value: 4 });
        MutableVecLockMut::apply_vec_diff(v, VecDiff::Clear {});
        MutableVecLockMut::apply_vec_diff(v, VecDiff::Replace { values: vec![0, 1, 2, 3] });
        MutableVecLockMut::apply_vec_diff(v, VecDiff::Move { old_index: 0, new_index: 2 });
        MutableVecLockMut::apply_vec_diff(v, VecDiff::UpdateAt { index: 1, value: 10 });
        MutableVecLockMut::apply_vec_diff(v, VecDiff::RemoveAt { index: 0 });
        MutableVecLockMut::apply_vec_diff(v, VecDiff::InsertAt { index: 0, value: 11 });
    }, vec![
        Poll::Pending,
        Poll::Ready(Some(VecDiff::Push { value: 6 })),
        Poll::Ready(Some(VecDiff::Pop {})),
        Poll::Ready(Some(VecDiff::Push { value: 4 })),
        Poll::Ready(Some(VecDiff::Clear {})),
        Poll::Ready(Some(VecDiff::Replace { values: vec![0, 1, 2, 3] })),
        Poll::Ready(Some(VecDiff::Move { old_index: 0, new_index: 2 })),
        Poll::Ready(Some(VecDiff::UpdateAt { index: 1, value: 10 })),
        Poll::Ready(Some(VecDiff::RemoveAt { index: 0 })),
        Poll::Ready(Some(VecDiff::InsertAt { index: 0, value: 11 })),
        Poll::Ready(None),
    ]);
}

// #[test]
// fn is_from_vec() {
//     let store = Store::new();
//     let src = vec![1,2,3];
//     let _out: MutableVec<u8> = store.create_mutable_vec_w_values(src);
// 
//     let src = vec![1,2,3];
//     let _out: MutableVec<u8> = src.into();
// 
//     let _out: MutableVec<u8> = [1,2,3].into();
// }