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
//! Stored as `Mutex<Option<Runtime>>` rather than `OnceLock<Runtime>` so the
//! PHP module-shutdown hook (`shutdown()` here, wired in `lib.rs`) can `take`
//! the runtime and drop it. Under module-unload SAPIs (mod_php, ZTS Apache)
//! the I/O threads would otherwise outlive `MSHUTDOWN` and leak.

use std::future::Future;
use std::sync::Mutex;
use std::time::Duration;

use tokio::runtime::{Handle, Runtime};

static RUNTIME: Mutex<Option<Runtime>> = Mutex::new(None);

/// Returns a `Handle` to the shared runtime, building it on first use.
///
/// The mutex is only held long enough to clone the cheap `Handle`; the actual
/// `block_on` runs outside the lock so requests do not serialize.
fn handle() -> Handle {
    let mut guard = RUNTIME.lock().expect("wreq-php runtime mutex poisoned");
    if guard.is_none() {
        *guard = Some(build());
    }
    guard
        .as_ref()
        .expect("runtime was just initialized")
        .handle()
        .clone()
}

/// Drives the future to completion on the shared runtime, blocking the
/// calling PHP thread.
pub fn block_on<F: Future>(future: F) -> F::Output {
    handle().block_on(future)
}

/// Module-shutdown hook: drops the runtime so its I/O threads terminate
/// before the SAPI tears down. A bounded shutdown timeout keeps a hung
/// connection from hanging PHP at module-unload time.
pub fn shutdown() {
    if let Some(rt) = RUNTIME
        .lock()
        .expect("wreq-php runtime mutex poisoned")
        .take()
    {
        rt.shutdown_timeout(Duration::from_secs(1));
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
