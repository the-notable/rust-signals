use std::task::Poll;
use rx_store::cancelable_future;
use futures_util::future::{ready, FutureExt};

mod util;


#[test]
fn test_cancelable_future() {
    let mut a = cancelable_future(ready(()), || ());

    util::with_noop_context(|cx| {
        assert_eq!(a.1.poll_unpin(cx), Poll::Ready(()));
    });
}
