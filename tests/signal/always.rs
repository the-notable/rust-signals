use std::task::Poll;
use rx_store::signal::{SignalExt, always};
use rx_store::store::RxStore;
use rx_store::traits::HasStoreHandle;
use crate::util;


#[test]
fn test_always() {
    let store = RxStore::new();
    let mut signal = always(1, store.store_handle().clone());

    util::with_noop_context(|cx| {
        assert_eq!(signal.poll_change_unpin(cx), Poll::Ready(Some(1)));
        assert_eq!(signal.poll_change_unpin(cx), Poll::Ready(None));
    });
}
