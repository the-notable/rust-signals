use std::task::Poll;
use rx_store::signal::SignalExt;
use rx_store::store::RxStore;
use rx_store::traits::HasStoreHandle;
use crate::util;


#[test]
fn test_eq() {
    let store = RxStore::new();
    
    let signal = util::Source::new(vec![
        Poll::Ready(true),
        Poll::Pending,
        Poll::Pending,
        Poll::Ready(false),
        Poll::Pending,
        Poll::Ready(true),
        Poll::Pending,
        Poll::Ready(false),
        Poll::Pending,
        Poll::Ready(true),
        Poll::Ready(false),
        Poll::Ready(true),
        Poll::Pending,
        Poll::Pending,
        Poll::Pending,
        Poll::Ready(false),
        Poll::Ready(true),
        Poll::Pending,
        Poll::Pending,
        Poll::Pending,
        Poll::Ready(false),
        Poll::Ready(true),
        Poll::Pending,
        Poll::Ready(false),
        Poll::Pending,
    ], store.store_handle().clone());

    let stream = util::Source::<u32>::new(vec![
        Poll::Pending,
        Poll::Pending,
        Poll::Pending,
        Poll::Pending,
        Poll::Ready(1),
        Poll::Pending,
        Poll::Pending,
        Poll::Ready(5),
        Poll::Pending,
        Poll::Ready(3),
        Poll::Pending,
        Poll::Pending,
        Poll::Ready(1),
        Poll::Ready(2),
        Poll::Ready(3),
        Poll::Pending,
        Poll::Pending,
        Poll::Pending,
        Poll::Pending,
        Poll::Pending,
        Poll::Ready(1),
        Poll::Ready(1),
        Poll::Pending,
        Poll::Pending,
        Poll::Pending,
        Poll::Pending,
        Poll::Pending,
        Poll::Pending,
        Poll::Ready(0),
        Poll::Ready(1),
        Poll::Pending,
        Poll::Ready(3),
        Poll::Pending,
    ], store.store_handle().clone());

    let output = signal.sample_stream_cloned(stream);

    util::assert_stream_eq(output, vec![
        Poll::Pending,
        Poll::Pending,
        Poll::Pending,
        Poll::Pending,
        Poll::Ready(Some((true, 1))),
        Poll::Pending,
        Poll::Pending,
        Poll::Ready(Some((true, 5))),
        Poll::Pending,
        Poll::Ready(Some((false, 3))),
        Poll::Pending,
        Poll::Pending,
        Poll::Ready(Some((true, 1))),
        Poll::Ready(Some((false, 2))),
        Poll::Ready(Some((true, 3))),
        Poll::Pending,
        Poll::Pending,
        Poll::Pending,
        Poll::Pending,
        Poll::Pending,
        Poll::Ready(Some((true, 1))),
        Poll::Ready(Some((true, 1))),
        Poll::Pending,
        Poll::Pending,
        Poll::Pending,
        Poll::Pending,
        Poll::Pending,
        Poll::Pending,
        Poll::Ready(Some((true, 0))),
        Poll::Ready(Some((true, 1))),
        Poll::Pending,
        Poll::Ready(Some((true, 3))),
        Poll::Pending,
        Poll::Ready(None)
    ]);
}
