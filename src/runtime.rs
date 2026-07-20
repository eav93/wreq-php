//! Process-global Tokio runtime.
//!
//! `wreq` is async; PHP is synchronous. Every request bridges the two with a
//! single `block_on`. Unlike `php-rnet` (which builds a `Runtime` per client),
//! we share **one** multi-thread runtime for the whole process so that creating
//! many `Client` objects stays cheap.
//!
//! The runtime is initialized lazily on the first request. In php-fpm this
//! happens inside an already-forked worker, so the I/O threads are spawned in
//! the worker — never in the master.
//!
//! ## Fork safety
//!
//! A Tokio runtime does not survive `fork(2)`: only the calling thread is
//! cloned, so the I/O driver's kqueue/epoll descriptor and the worker threads
//! are left dangling in the child. Touching such a runtime (or dropping it)
//! aborts the process with `failed to wake I/O driver: Bad file descriptor`.
//! This bites any app that itself calls `pcntl_fork()` after a request — e.g. a
//! CLI that primes an HTTP call in the parent and then forks workers.
//!
//! We tag the cell with the PID that built the runtime. `current()` rebuilds a
//! fresh runtime whenever it sees a different PID (we are in a forked child),
//! and both the rebuild path and the shutdown hook `mem::forget` the inherited
//! runtime instead of dropping it — its threads/fds belong to the parent and
//! its `Drop` would panic here. The leak is one runtime struct in a
//! short-lived child, which exits soon after.
//!
//! Stored as `Mutex<Option<(u32, Arc<Runtime>)>>` so the PHP module-shutdown
//! hook (`shutdown()` here, wired in `lib.rs`) can `take` the cell and drop the
//! runtime. Wrapping in `Arc` lets each `block_on` call grab an owned
//! reference, drop the mutex, and then call `Runtime::block_on` outside the
//! lock — using the runtime API directly rather than `Handle::block_on`, which
//! empirically does not drive wreq's first request reliably from a freshly
//! built runtime.

use std::future::Future;
use std::mem;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::runtime::Runtime;

/// The shared runtime plus the PID of the process that built it (see the
/// module-level "Fork safety" note).
static RUNTIME: Mutex<Option<(u32, Arc<Runtime>)>> = Mutex::new(None);

/// Returns an owned reference to the shared runtime, building it on first use
/// and rebuilding it after a `fork` (when the stored PID no longer matches).
fn current() -> Arc<Runtime> {
    let pid = std::process::id();
    let mut guard = RUNTIME.lock().expect("wreq-php runtime mutex poisoned");

    match guard.as_ref() {
        Some((owner, rt)) if *owner == pid => return Arc::clone(rt),
        // Inherited across a fork: the runtime belongs to the parent process and
        // must not be dropped here. Abandon it without running Drop, then fall
        // through to build a fresh one for this process.
        Some(_) => {
            if let Some((_, stale)) = guard.take() {
                mem::forget(stale);
            }
        }
        None => {}
    }

    let rt = Arc::new(build());
    *guard = Some((pid, Arc::clone(&rt)));
    rt
}

/// Drives the future to completion on the shared runtime, blocking the
/// calling PHP thread.
pub fn block_on<F: Future>(future: F) -> F::Output {
    current().block_on(future)
}

/// Module-shutdown hook: drops the runtime so its I/O threads terminate
/// before the SAPI tears down. A bounded shutdown timeout keeps a hung
/// connection from hanging PHP at module-unload time. If a `block_on` is
/// still in flight when this runs, its `Arc` keeps the `Runtime` alive long
/// enough to finish — we only release the static slot.
pub fn shutdown() {
    let pid = std::process::id();
    let taken = RUNTIME
        .lock()
        .expect("wreq-php runtime mutex poisoned")
        .take();
    if let Some((owner, arc)) = taken {
        // Runtime inherited from a parent across fork — its threads/fds are not
        // ours; dropping it would panic. Leak it instead (the child is exiting).
        if owner != pid {
            mem::forget(arc);
            return;
        }
        if let Ok(rt) = Arc::try_unwrap(arc) {
            rt.shutdown_timeout(Duration::from_secs(1));
        }
        // If another thread still holds an Arc, Runtime drops itself when
        // that last reference goes away — close enough at module unload.
    }
}

fn build() -> Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(worker_threads())
        .enable_all()
        .thread_name("wreq-php-rt")
        .build()
        .expect("failed to build the wreq-php Tokio runtime")
}

/// Number of Tokio worker threads per process.
///
/// PHP is synchronous — at most one `block_on` runs at a time per worker — so a
/// single thread is enough to drive a request while still letting the runtime
/// keep idle keep-alive connections warm in the background (which a
/// `current_thread` runtime could not do between requests). Under php-fpm every
/// worker is its own process with its own runtime, so each extra thread is paid
/// for N workers over: keeping the default at 1 avoids hundreds of idle threads
/// on busy pools. `WREQ_PHP_WORKER_THREADS` lets heavily-concurrent setups tune
/// it up without a rebuild.
fn worker_threads() -> usize {
    std::env::var("WREQ_PHP_WORKER_THREADS")
        .ok()
        .and_then(|v| v.trim().parse::<usize>().ok())
        .filter(|&n| n > 0)
        .unwrap_or(1)
}
