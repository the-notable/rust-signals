use std::task::Poll;

use futures_executor::block_on_stream;

use rx_store::map_ref;
use rx_store::signal::{always, Broadcaster, from_stream, SignalExt};
use rx_store::store::{Manager, RxStore};
use rx_store::traits::{HasSignal, HasStoreHandle};

mod util;


#[test]
fn test_broadcaster() {
    let store = RxStore::new();
    
    let mutable = store.new_mutable(1);
    let broadcaster = mutable.signal().broadcast();
    //let broadcaster = Broadcaster::new(mutable.signal());
    let mut b1 = broadcaster.signal();
    //let mut b2 = broadcaster.signal_cloned();

    util::with_noop_context(|cx| {
        assert_eq!(b1.poll_change_unpin(cx), Poll::Ready(Some(1)));
        assert_eq!(b1.poll_change_unpin(cx), Poll::Pending);
        // assert_eq!(b2.poll_change_unpin(cx), Poll::Ready(Some(1)));
        // assert_eq!(b2.poll_change_unpin(cx), Poll::Pending);

        mutable.set(5);
        assert_eq!(b1.poll_change_unpin(cx), Poll::Ready(Some(5)));
        assert_eq!(b1.poll_change_unpin(cx), Poll::Pending);
        // assert_eq!(b2.poll_change_unpin(cx), Poll::Ready(Some(5)));
        // assert_eq!(b2.poll_change_unpin(cx), Poll::Pending);

        mutable.set(6);
        drop(mutable);
        assert_eq!(b1.poll_change_unpin(cx), Poll::Ready(Some(6)));
        assert_eq!(b1.poll_change_unpin(cx), Poll::Ready(None));
        // assert_eq!(b2.poll_change_unpin(cx), Poll::Ready(Some(6)));
        // assert_eq!(b2.poll_change_unpin(cx), Poll::Ready(None));
    });
}

#[test]
fn test_polls() {
    let store = RxStore::new();
    let mutable = store.new_mutable(1);
    let broadcaster = mutable.signal().broadcast();
    //let broadcaster = Broadcaster::new(mutable.signal());
    let signal1 = broadcaster.signal();
    let signal2 = broadcaster.signal();

    let mut mutable = Some(mutable);
    let mut broadcaster = Some(broadcaster);

    let polls = util::get_all_polls(map_ref!(signal1, signal2 => (*signal1, *signal2)), 0, |state, cx| {
        match *state {
            0 => {},
            1 => { cx.waker().wake_by_ref(); },
            2 => { mutable.as_ref().unwrap().set(5); },
            3 => { cx.waker().wake_by_ref(); },
            4 => { mutable.take(); },
            5 => { broadcaster.take(); },
            _ => {},
        }

        state + 1
    });

    assert_eq!(polls, vec![
        Poll::Ready(Some((1, 1))),
        Poll::Pending,
        Poll::Ready(Some((5, 5))),
        Poll::Pending,
        Poll::Ready(None),
    ]);
}


#[test]
fn test_broadcaster_signal_ref() {
    let store = RxStore::new();
    let signal1 = always(1, store.store_handle().clone());
    let broadcaster = signal1.broadcast();
    //let broadcaster = Broadcaster::new(always(1, store.store_handle().clone()));
    let mut signal2 = broadcaster.signal_ref(|x| x + 5);
    util::with_noop_context(|cx| {
        assert_eq!(signal2.poll_change_unpin(cx), Poll::Ready(Some(6)));
        assert_eq!(signal2.poll_change_unpin(cx), Poll::Ready(None));
    });
}


#[test]
fn test_broadcaster_always() {
    let store = RxStore::new();
    let signal = always(1, store.store_handle().clone());
    let broadcaster = signal.broadcast();
    //let broadcaster = Broadcaster::new(always(1, store.store_handle().clone()));
    let mut signal = broadcaster.signal();
    util::with_noop_context(|cx| {
        assert_eq!(signal.poll_change_unpin(cx), Poll::Ready(Some(1)));
        assert_eq!(signal.poll_change_unpin(cx), Poll::Ready(None));
    });
}


#[test]
fn test_broadcaster_drop() {
    let store = RxStore::new();
    let mutable = store.new_mutable(1);
    let broadcaster = mutable.signal().broadcast();
    //let broadcaster = Broadcaster::new(mutable.signal());
    let mut signal = broadcaster.signal();
    util::with_noop_context(|cx| {
        assert_eq!(signal.poll_change_unpin(cx), Poll::Ready(Some(1)));
        drop(mutable);
        assert_eq!(signal.poll_change_unpin(cx), Poll::Ready(None));
    });
}


#[test]
fn test_broadcaster_multiple() {
    let store = RxStore::new();
    let mutable = store.new_mutable(1);
    let broadcaster = mutable.signal().broadcast();
    //let broadcaster = Broadcaster::new(mutable.signal());
    let mut signal1 = broadcaster.signal();
    let mut signal2 = broadcaster.signal();
    drop(mutable);
    util::with_noop_context(|cx| {
        assert_eq!(signal1.poll_change_unpin(cx), Poll::Ready(Some(1)));
        assert_eq!(signal1.poll_change_unpin(cx), Poll::Ready(None));
        assert_eq!(signal2.poll_change_unpin(cx), Poll::Ready(Some(1)));
        assert_eq!(signal2.poll_change_unpin(cx), Poll::Ready(None));
    });
}


#[test]
fn test_block_on_stream() {
    let store = RxStore::new();
    let mutable = store.new_mutable(1);
    let signal_from_stream = mutable.signal();
    
    let broadcaster = signal_from_stream.broadcast();
    let mut blocking_stream = block_on_stream(broadcaster.signal().to_stream());

    assert_eq!(blocking_stream.next(), Some(1));
    mutable.set(2);

    assert_eq!(blocking_stream.next(), Some(2));
}

#[test]
fn test_block_on_stream_wrapper() {
    let store = RxStore::new();
    let mutable = store.new_mutable(1);
    let signal_from_stream = from_stream(
        mutable.signal().to_stream(),
        store.store_handle().clone()
    );
    
    let broadcaster = signal_from_stream.broadcast();
    let mut blocking_stream = block_on_stream(broadcaster.signal().debug().to_stream());

    assert_eq!(blocking_stream.next().unwrap(), Some(1));
    mutable.set(2);

    assert_eq!(blocking_stream.next().unwrap(), Some(2));
}
