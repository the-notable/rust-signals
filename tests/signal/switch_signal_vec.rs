use std::task::Poll;
use rx_store::signal::SignalExt;
use rx_store::signal_vec::VecDiff;
use rx_store::store::RxStore;
use rx_store::traits::HasStoreHandle;
use crate::util;


#[test]
fn test_switch_signal_vec() {
    let store = RxStore::new();
    let input = util::Source::new(vec![
        Poll::Ready(true),
        Poll::Pending,
        Poll::Pending,
        Poll::Ready(false),
        Poll::Ready(false),
        Poll::Pending,
        Poll::Pending,
        Poll::Pending,
        Poll::Pending,
        Poll::Pending,
        Poll::Pending,
        Poll::Ready(false),
        Poll::Ready(true),
        Poll::Pending,
    ], store.store_handle().clone());

    let output = input.switch_signal_vec(move |test| {
        if test {
            util::Source::new(vec![
                Poll::Ready(VecDiff::Push { value: 10 }),
            ], store.store_handle().clone())

        } else {
            util::Source::new(vec![
                Poll::Ready(VecDiff::Replace { values: vec![0, 1, 2, 3, 4, 5] }),
                Poll::Ready(VecDiff::Push { value: 6 }),
                Poll::Pending,
                Poll::Pending,
                Poll::Ready(VecDiff::InsertAt { index: 0, value: 7 }),
            ], store.store_handle().clone())
        }
    });

    util::assert_signal_vec_eq(output, vec![
        Poll::Ready(Some(VecDiff::Push { value: 10 })),
        Poll::Pending,
        Poll::Ready(Some(VecDiff::Replace { values: vec![0, 1, 2, 3, 4, 5] })),
        Poll::Ready(Some(VecDiff::Push { value: 6 })),
        Poll::Pending,
        Poll::Pending,
        Poll::Ready(Some(VecDiff::InsertAt { index: 0, value: 7 })),
        Poll::Pending,
        Poll::Ready(Some(VecDiff::Replace { values: vec![10] })),
        Poll::Ready(None),
    ]);
}


#[test]
fn test_switch_signal_vec_bug() {
    let store = RxStore::new();
    let input = util::Source::new(vec![
        Poll::Ready(util::Source::new(vec![
            Poll::Ready(vec![]),
            Poll::Pending,
            Poll::Ready(vec!["hello", "world"]),
        ], store.store_handle().clone())),
    ], store.store_handle().clone());

    let output = input.switch_signal_vec(|messages| messages.to_signal_vec());

    util::assert_signal_vec_eq(output, vec![
        Poll::Ready(Some(VecDiff::Replace { values: vec![] })),
        Poll::Pending,
        Poll::Ready(Some(VecDiff::Replace { values: vec!["hello", "world"] })),
        Poll::Ready(None),
    ]);
}
