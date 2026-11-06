//! Work that must not run on the game coordinator. The coordinator owns the join/leave, packet
//! and clock loop; anything that only needs a snapshot (the locator bar is O(players²)) is handed
//! to a worker thread through a one-slot mailbox: submitting never blocks, and when the worker
//! falls behind the newest snapshot replaces the one it has not started yet.

use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;

struct Slot<T> {
    next: Option<T>,
    closed: bool,
}

pub struct Latest<T> {
    shared: Arc<(Mutex<Slot<T>>, Condvar)>,
    worker: Option<JoinHandle<()>>,
}

impl<T: Send + 'static> Latest<T> {
    /// Starts a worker thread that runs `work` on the newest submitted value.
    pub fn spawn(name: &str, mut work: impl FnMut(T) + Send + 'static) -> Self {
        let shared = Arc::new((
            Mutex::new(Slot {
                next: None,
                closed: false,
            }),
            Condvar::new(),
        ));
        let handle = Arc::clone(&shared);
        let worker = std::thread::Builder::new()
            .name(name.to_owned())
            .spawn(move || {
                let (lock, wake) = &*handle;
                loop {
                    let value = {
                        let mut slot = lock.lock().unwrap_or_else(|e| e.into_inner());
                        loop {
                            if let Some(value) = slot.next.take() {
                                break value;
                            }
                            if slot.closed {
                                return;
                            }
                            slot = wake.wait(slot).unwrap_or_else(|e| e.into_inner());
                        }
                    };
                    work(value);
                }
            })
            .expect("spawning an offload worker");
        Self {
            shared,
            worker: Some(worker),
        }
    }

    /// Replaces the pending value; returns whether an older one was dropped unprocessed.
    pub fn submit(&self, value: T) -> bool {
        let (lock, wake) = &*self.shared;
        let replaced = lock
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .next
            .replace(value)
            .is_some();
        wake.notify_one();
        replaced
    }
}

impl<T> Drop for Latest<T> {
    fn drop(&mut self) {
        let (lock, wake) = &*self.shared;
        lock.lock().unwrap_or_else(|e| e.into_inner()).closed = true;
        wake.notify_one();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    #[test]
    fn a_slow_worker_sees_only_the_newest_pending_value() {
        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel::<()>();
        let (done_tx, done_rx) = mpsc::channel();
        let release_rx = Mutex::new(release_rx);
        let latest = Latest::spawn("test-offload", move |value: u32| {
            started_tx.send(value).unwrap();
            release_rx.lock().unwrap().recv().unwrap();
            done_tx.send(value).unwrap();
        });
        assert!(!latest.submit(1));
        assert_eq!(started_rx.recv().unwrap(), 1);
        // The worker is busy with 1: 2 is dropped in favour of 3, and submitting never blocks.
        assert!(!latest.submit(2));
        assert!(latest.submit(3));
        release_tx.send(()).unwrap();
        assert_eq!(done_rx.recv().unwrap(), 1);
        assert_eq!(started_rx.recv().unwrap(), 3);
        release_tx.send(()).unwrap();
        assert_eq!(done_rx.recv().unwrap(), 3);
        drop(latest);
        assert!(done_rx.try_recv().is_err());
    }
}
