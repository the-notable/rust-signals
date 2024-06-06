use std::task::Poll;
use rx_store::signal::SignalExt;
use rx_store::store::RxStore;
use rx_store::traits::HasStoreHandle;
use crate::util;


#[test]
fn test_stop_if() {
    let store = RxStore::new();
    let input = util::Source::new(vec![
        Poll::Ready(0),
        Poll::Pending,
        Poll::Pending,
        Poll::Ready(1),
        Poll::Ready(5),
        Poll::Pending,
        Poll::Pending,
        Poll::Pending,
        Poll::Pending,
        Poll::Pending,
        Poll::Pending,
        Poll::Ready(0),
        Poll::Pending,
        Poll::Ready(3),
        Poll::Pending,
    ], store.store_handle().clone());

    let output = input.stop_if(move |x| *x == 5);

    util::assert_signal_eq(output, vec![
        Poll::Ready(Some(0)),
        Poll::Pending,
        Poll::Pending,
        Poll::Ready(Some(1)),
        Poll::Ready(Some(5)),
        Poll::Ready(None),
    ]);
}
