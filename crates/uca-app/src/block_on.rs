// SPDX-FileCopyrightText: The UCA contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A minimal `block_on` for egui-wgpu's async setup (05, decided: no async runtime).

use std::future::Future;
use std::pin::pin;
use std::sync::Arc;
use std::task::{Context, Poll, Wake, Waker};
use std::thread::{self, Thread};

struct ThreadWaker(Thread);

impl Wake for ThreadWaker {
    fn wake(self: Arc<Self>) {
        self.0.unpark();
    }
}

/// Runs a future to completion on the current thread.
pub fn block_on<F: Future>(future: F) -> F::Output {
    let mut future = pin!(future);
    let waker = Waker::from(Arc::new(ThreadWaker(thread::current())));
    let mut cx = Context::from_waker(&waker);
    loop {
        match future.as_mut().poll(&mut cx) {
            Poll::Ready(v) => return v,
            Poll::Pending => thread::park(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runs_ready_and_pending_futures() {
        assert_eq!(block_on(async { 7 }), 7);
        let (tx, rx) = std::sync::mpsc::channel::<u32>();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(20));
            let _ = tx.send(3);
        });
        // A future that is pending until the other thread has sent.
        let got = block_on(std::future::poll_fn(|cx| match rx.try_recv() {
            Ok(v) => Poll::Ready(v),
            Err(_) => {
                cx.waker().wake_by_ref();
                Poll::Pending
            }
        }));
        assert_eq!(got, 3);
    }
}
