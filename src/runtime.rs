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

use std::sync::OnceLock;

use tokio::runtime::Runtime;

static RUNTIME: OnceLock<Runtime> = OnceLock::new();

/// Returns the shared runtime, building it on first use.
pub fn runtime() -> &'static Runtime {
    RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .thread_name("wreq-php-rt")
            .build()
            .expect("failed to build the wreq-php Tokio runtime")
    })
}
