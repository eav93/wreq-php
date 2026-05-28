//! Process-global Tokio runtime.
//!
//! `wreq` is async; PHP is synchronous. Every request bridges the two with a
//! single `block_on`. Unlike `php-rnet` (which builds a `Runtime` per client),
//! we share **one** multi-thread runtime for the whole process so that creating
//! many `Client` objects stays cheap.
//!
//! The runtime is initialized lazily on the first request. In php-fpm this
//! happens inside an already-forked worker, so the I/O threads are spawned in
//! the worker — never in the master. Do not create a `Client` during module
//! load / MINIT.
//!
//! Stored as `Mutex<Option<Arc<Runtime>>>` so the PHP module-shutdown hook
//! (`shutdown()` here, wired in `lib.rs`) can `take` the cell and drop the
//! runtime. Under module-unload SAPIs (mod_php, ZTS Apache) the I/O threads
//! would otherwise outlive `MSHUTDOWN` and leak. Wrapping in `Arc` lets each
//! `block_on` call grab an owned reference, drop the mutex, and then call
//! `Runtime::block_on` outside the lock — using the runtime API directly
//! rather than `Handle::block_on`, which empirically does not drive wreq's
//! first request reliably from a freshly built runtime.

use std::future::Future;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::runtime::Runtime;

static RUNTIME: Mutex<Option<Arc<Runtime>>> = Mutex::new(None);

/// Returns an owned reference to the shared runtime, building it on first use.
fn current() -> Arc<Runtime> {
    let mut guard = RUNTIME.lock().expect("wreq-php runtime mutex poisoned");
    if guard.is_none() {
        *guard = Some(Arc::new(build()));
    }
    Arc::clone(guard.as_ref().expect("runtime was just initialized"))
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
    let arc = RUNTIME
        .lock()
        .expect("wreq-php runtime mutex poisoned")
        .take();
    if let Some(arc) = arc {
        if let Ok(rt) = Arc::try_unwrap(arc) {
            rt.shutdown_timeout(Duration::from_secs(1));
        }
        // If another thread still holds an Arc, Runtime drops itself when
        // that last reference goes away — close enough at module unload.
    }
}

fn build() -> Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .thread_name("wreq-php-rt")
        .build()
        .expect("failed to build the wreq-php Tokio runtime")
}
