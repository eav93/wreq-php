//! wreq-php — PHP HTTP client built on `wreq`.
//!
//! This crate is the native extension: a thin, fast core that owns the
//! `wreq::Client` and its connection pool. The Laravel-style ergonomics
//! (`json()`, `successful()`, immutable per-request builders, …) live in the
//! companion PHP Composer package, which wraps the classes registered here.
//!
//! Registered PHP classes:
//! * `Wreq\Ext\Client`   — reusable client with a dedicated connection pool.
//! * `Wreq\Ext\Response` — raw response (status, headers, body bytes).
//! * `Wreq\Ext\Emulation` — registry of browser emulation profiles.
//! * `Wreq\Ext\RequestException` (+ subclasses) — error hierarchy.

// PHP extensions on Windows use the `vectorcall` ABI; the `ext-php-rs` macros
// expand `extern "vectorcall"` into this crate, so the still-unstable feature
// must be enabled here too. Windows therefore needs a nightly compiler; Linux
// and macOS are unaffected and build on stable.
#![cfg_attr(windows, feature(abi_vectorcall))]
#![allow(clippy::new_without_default)]

use ext_php_rs::prelude::*;

mod client;
mod convert;
mod emulation;
mod error;
mod response;
mod runtime;

use client::Client;
use emulation::Emulation;
use response::Response;

#[php_module]
pub fn get_module(module: ModuleBuilder) -> ModuleBuilder {
    // Exception classes first: the base `RequestException` must be registered
    // before the subclasses that extend it.
    let module = error::exception_classes(module);

    module
        .class::<Client>()
        .class::<Response>()
        .class::<Emulation>()
}
