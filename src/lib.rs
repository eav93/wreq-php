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
//! * `Wreq\Ext\WreqException` (+ subclasses) — error hierarchy.

// PHP extensions on Windows use the `vectorcall` ABI; the `ext-php-rs` macros
// expand `extern "vectorcall"` into this crate, so the still-unstable feature
// must be enabled here too. Windows therefore needs a nightly compiler; Linux
// and macOS are unaffected and build on stable.
#![cfg_attr(windows, feature(abi_vectorcall))]
#![allow(clippy::new_without_default)]

use ext_php_rs::prelude::*;
use ext_php_rs::zend::ModuleEntry;
use ext_php_rs::{info_table_end, info_table_row, info_table_start};
use strum::VariantArray;

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
    // Exception classes first: the base `WreqException` must be registered
    // before the subclasses that extend it.
    let module = error::exception_classes(module);

    module
        .info_function(module_info)
        .class::<Client>()
        .class::<Response>()
        .class::<Emulation>()
}

/// Renders the wreq-php block shown by PHP's `module_info()`.
///
/// Lists the extension version (the same value `Client::extension_version()`
/// returns), the linked `wreq` crate version, the number of emulation profiles
/// shipped by the bundled `wreq-util`, and which compression/transport features
/// are compiled in. Useful for confirming "which build am I actually running?"
/// without booting a PHP interpreter and constructing a client.
#[unsafe(no_mangle)]
pub extern "C" fn module_info(_module: *mut ModuleEntry) {
    let profiles = wreq_util::Emulation::VARIANTS.len().to_string();

    info_table_start!();
    info_table_row!("wreq-php", "enabled");
    info_table_row!("Extension version", env!("WREQ_PHP_BUILD_VERSION"));
    info_table_row!("wreq crate version", WREQ_CRATE_VERSION);
    info_table_row!("Emulation profiles", profiles.as_str());
    info_table_row!("Compression", "gzip, brotli, deflate, zstd");
    info_table_row!("DNS resolver", "hickory-dns");
    info_table_end!();
}

/// Compile-time `wreq` crate version, extracted from `Cargo.lock` by the build
/// script — `CARGO_PKG_VERSION` would only give us our own version.
const WREQ_CRATE_VERSION: &str = env!("WREQ_CRATE_VERSION");
