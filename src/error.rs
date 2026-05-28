//! Error types and the exception hierarchy exposed to PHP.
//!
//! All transport errors map onto a `Wreq\Ext\WreqException` subtree so PHP
//! code can `catch (\Wreq\Ext\WreqException)` to handle any failure, or a
//! specific subclass to react to one cause.

use ext_php_rs::class::RegisteredClass;
use ext_php_rs::exception::PhpException;
use ext_php_rs::prelude::*;
use ext_php_rs::zend::{ce, ClassEntry};

// ---------- exception hierarchy ----------

/// Base class for every error raised by the extension. Extends `\Exception`.
#[php_class]
#[php(name = "Wreq\\Ext\\WreqException")]
#[php(extends(ce = ce::exception, stub = "\\Exception"))]
#[derive(Default)]
pub struct WreqException;

#[php_impl]
impl WreqException {}

/// Class-entry accessor so the subclasses below can extend `WreqException`.
///
/// The short `extends(WreqException)` form is documented in
/// `macros/classes.md` but in ext-php-rs-derive 0.11.13 it emits the parent's
/// `CLASS_NAME` ("Wreq\\Ext\\WreqException") as the stub path *without* a
/// leading backslash. Inside the `namespace Wreq\Ext { … }` block PHP then
/// treats that as relative — resolving to `Wreq\Ext\Wreq\Ext\WreqException`
/// — and PhpStorm flags the hierarchy as broken. The explicit form gives us
/// a correct FQN in `Wreq.stubs.php` so we keep this helper until the upstream
/// macro is fixed.
fn wreq_exception_ce() -> &'static ClassEntry {
    <WreqException as RegisteredClass>::get_metadata().ce()
}

/// Connection could not be established (DNS, refused, reset).
#[php_class]
#[php(name = "Wreq\\Ext\\ConnectionException")]
#[php(extends(ce = wreq_exception_ce, stub = "\\Wreq\\Ext\\WreqException"))]
#[derive(Default)]
pub struct ConnectionException;

#[php_impl]
impl ConnectionException {}

/// Request exceeded its timeout.
#[php_class]
#[php(name = "Wreq\\Ext\\TimeoutException")]
#[php(extends(ce = wreq_exception_ce, stub = "\\Wreq\\Ext\\WreqException"))]
#[derive(Default)]
pub struct TimeoutException;

#[php_impl]
impl TimeoutException {}

/// TLS handshake / certificate failure.
#[php_class]
#[php(name = "Wreq\\Ext\\TlsException")]
#[php(extends(ce = wreq_exception_ce, stub = "\\Wreq\\Ext\\WreqException"))]
#[derive(Default)]
pub struct TlsException;

#[php_impl]
impl TlsException {}

/// Redirect policy was violated (loop or limit exceeded).
#[php_class]
#[php(name = "Wreq\\Ext\\RedirectException")]
#[php(extends(ce = wreq_exception_ce, stub = "\\Wreq\\Ext\\WreqException"))]
#[derive(Default)]
pub struct RedirectException;

#[php_impl]
impl RedirectException {}

/// Every exception class, in registration order (parent first).
pub fn exception_classes(module: ModuleBuilder) -> ModuleBuilder {
    module
        .class::<WreqException>()
        .class::<ConnectionException>()
        .class::<TimeoutException>()
        .class::<TlsException>()
        .class::<RedirectException>()
}

// ---------- internal error ----------

/// A simple internal error carrying a human-readable message. Request building
/// (e.g. header validation) reports failures through this; transport errors go
/// through `map_wreq_error` directly.
#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct Error(String);

impl Error {
    pub fn other(msg: impl Into<String>) -> Self {
        Error(msg.into())
    }
}

impl From<Error> for PhpException {
    fn from(e: Error) -> Self {
        PhpException::from_class::<WreqException>(e.to_string())
    }
}

/// Maps a `wreq::Error` onto the most specific PHP exception class.
pub fn map_wreq_error(e: wreq::Error) -> PhpException {
    let msg = e.to_string();
    if e.is_timeout() {
        PhpException::from_class::<TimeoutException>(msg)
    } else if e.is_tls() {
        // Checked before `is_connect`: a TLS handshake failure is also a
        // connect failure, but `TlsException` is the more useful class.
        PhpException::from_class::<TlsException>(msg)
    } else if e.is_connect() {
        PhpException::from_class::<ConnectionException>(msg)
    } else if e.is_redirect() {
        PhpException::from_class::<RedirectException>(msg)
    } else {
        PhpException::from_class::<WreqException>(msg)
    }
}

pub type Result<T> = std::result::Result<T, Error>;
