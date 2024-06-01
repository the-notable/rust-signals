use std::task::Poll;
use rx_store::cancelable_future;
use rx_store::signal::{SignalExt, Mutable, channel};
use rx_store::store::{Manager, Store};
use rx_store::traits::{HasSignal, HasSignalCloned};
use crate::util;


#[test]
fn test_mutable() {
    let store = Store::new();
    let mutable = store.create_mutable(1);
    let mut s1 = mutable.signal();
    let mut s2 = mutable.signal_cloned();

    util::with_noop_context(|cx| {
        assert_eq!(s1.poll_change_unpin(cx), Poll::Ready(Some(1)));
        assert_eq!(s1.poll_change_unpin(cx), Poll::Pending);
        assert_eq!(s2.poll_change_unpin(cx), Poll::Ready(Some(1)));
        assert_eq!(s2.poll_change_unpin(cx), Poll::Pending);

        mutable.set(5);
        assert_eq!(s1.poll_change_unpin(cx), Poll::Ready(Some(5)));
        assert_eq!(s1.poll_change_unpin(cx), Poll::Pending);
        assert_eq!(s2.poll_change_unpin(cx), Poll::Ready(Some(5)));
        assert_eq!(s2.poll_change_unpin(cx), Poll::Pending);

        drop(mutable);
        assert_eq!(s1.poll_change_unpin(cx), Poll::Ready(None));
        assert_eq!(s2.poll_change_unpin(cx), Poll::Ready(None));
    });
}

#[test]
fn test_mutable_drop() {

    let store = Store::new();
    
    {
        let mutable = store.create_mutable(1);
        let mut s1 = mutable.signal();
        let mut s2 = mutable.signal_cloned();
        drop(mutable);

        util::with_noop_context(|cx| {
            assert_eq!(s1.poll_change_unpin(cx), Poll::Ready(Some(1)));
            assert_eq!(s1.poll_change_unpin(cx), Poll::Ready(None));
            assert_eq!(s2.poll_change_unpin(cx), Poll::Ready(Some(1)));
            assert_eq!(s2.poll_change_unpin(cx), Poll::Ready(None));
        });
    }

    {
        let mutable = store.create_mutable(1);
        let mut s1 = mutable.signal();
        let mut s2 = mutable.signal_cloned();

        util::with_noop_context(|cx| {
            assert_eq!(s1.poll_change_unpin(cx), Poll::Ready(Some(1)));
            assert_eq!(s1.poll_change_unpin(cx), Poll::Pending);
            assert_eq!(s2.poll_change_unpin(cx), Poll::Ready(Some(1)));
            assert_eq!(s2.poll_change_unpin(cx), Poll::Pending);

            mutable.set(5);
            drop(mutable);

            assert_eq!(s1.poll_change_unpin(cx), Poll::Ready(Some(5)));
            assert_eq!(s1.poll_change_unpin(cx), Poll::Ready(None));
            assert_eq!(s2.poll_change_unpin(cx), Poll::Ready(Some(5)));
            assert_eq!(s2.poll_change_unpin(cx), Poll::Ready(None));
        });
    }

    {
        let mutable = store.create_mutable(1);
        let mut s1 = mutable.signal();
        let mut s2 = mutable.signal_cloned();

        util::with_noop_context(|cx| {
            assert_eq!(s1.poll_change_unpin(cx), Poll::Ready(Some(1)));
            assert_eq!(s1.poll_change_unpin(cx), Poll::Pending);
            assert_eq!(s2.poll_change_unpin(cx), Poll::Ready(Some(1)));
            assert_eq!(s2.poll_change_unpin(cx), Poll::Pending);

            mutable.set(5);
            assert_eq!(s1.poll_change_unpin(cx), Poll::Ready(Some(5)));
            assert_eq!(s1.poll_change_unpin(cx), Poll::Pending);

            drop(mutable);
            assert_eq!(s2.poll_change_unpin(cx), Poll::Ready(Some(5)));
            assert_eq!(s2.poll_change_unpin(cx), Poll::Ready(None));

            assert_eq!(s1.poll_change_unpin(cx), Poll::Ready(None));
        });
    }
}

#[test]
fn test_send_sync() {
    let store = Store::new();
    
    let a = cancelable_future(async {}, || ());
    let _: Box<dyn Send + Sync> = Box::new(a.0);
    let _: Box<dyn Send + Sync> = Box::new(a.1);

    let _: Box<dyn Send + Sync> = Box::new(store.create_mutable(1));
    let _: Box<dyn Send + Sync> = Box::new(store.create_mutable(1).signal());
    let _: Box<dyn Send + Sync> = Box::new(store.create_mutable(1).signal_cloned());

    let a = channel(1);
    let _: Box<dyn Send + Sync> = Box::new(a.0);
    let _: Box<dyn Send + Sync> = Box::new(a.1);
}

// Verifies that lock_mut only notifies when it is mutated
#[test]
fn test_lock_mut() {
    let store = Store::new();
    
    {
        let m = store.create_mutable(1);

        let polls = util::get_signal_polls(m.signal(), move || {
            let mut lock = m.lock_mut();

            if *lock == 2 {
                *lock = 5;
            }
        });

        assert_eq!(polls, vec![
            Poll::Ready(Some(1)),
            Poll::Pending,
            Poll::Ready(None),
        ]);
    }

    {
        let m = store.create_mutable(1);

        let polls = util::get_signal_polls(m.signal(), move || {
            let mut lock = m.lock_mut();

            if *lock == 1 {
                *lock = 5;
            }
        });

        assert_eq!(polls, vec![
            Poll::Ready(Some(1)),
            Poll::Pending,
            Poll::Ready(Some(5)),
            Poll::Ready(None),
        ]);
    }
}


/*#[test]
fn test_lock_panic() {
    struct Foo;

    impl Foo {
        fn bar<A>(self, _value: &A) -> Self {
            self
        }

        fn qux<A>(self, _value: A) -> Self {
            self
        }
    }

    let m = Mutable::new(1);

    Foo
        .bar(&m.lock_ref())
        .qux(m.signal().map(move |x| x * 10));
}


#[test]
fn test_lock_mut_signal() {
    let m = Mutable::new(1);

    let mut output = {
        let mut lock = m.lock_mut();
        let output = lock.signal().map(move |x| x * 10);
        *lock = 2;
        output
    };

    util::with_noop_context(|cx| {
        assert_eq!(output.poll_change_unpin(cx), Poll::Ready(Some(20)));
        assert_eq!(output.poll_change_unpin(cx), Poll::Pending);
        assert_eq!(output.poll_change_unpin(cx), Poll::Pending);

        m.set(5);

        assert_eq!(output.poll_change_unpin(cx), Poll::Ready(Some(50)));
        assert_eq!(output.poll_change_unpin(cx), Poll::Pending);
        assert_eq!(output.poll_change_unpin(cx), Poll::Pending);

        drop(m);

        assert_eq!(output.poll_change_unpin(cx), Poll::Ready(None));
        assert_eq!(output.poll_change_unpin(cx), Poll::Ready(None));
    });
}*/

// #[test]
// fn is_from_t(){
//     let store = Store::new();
//     
//     let src = 0;
//     let _out: Mutable<u8> = store.create_mutable(src);
// 
//     let src = 0;
//     let _out: Mutable<u8> = src.into();
// }