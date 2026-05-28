//! Raw HTTP response exposed to PHP.
//!
//! This is deliberately thin: it exposes status, headers and the body as raw
//! bytes. All the Laravel-style sugar (`json()`, `object()`, `successful()`,
//! `resource()`, …) lives in the PHP layer, which wraps this object.
//!
//! The body is read eagerly when the response is constructed, so every getter
//! is idempotent and can be called any number of times.

use bytes::Bytes;
use ext_php_rs::prelude::*;
use ext_php_rs::types::{ZendStr, Zval};
use indexmap::IndexMap;

/// HTTP response returned by `Client::request()`.
#[php_class]
#[php(name = "Wreq\\Ext\\Response")]
pub struct Response {
    status: u16,
    version: String,
    url: String,
    /// Header lines in arrival order, original casing, duplicates kept.
    headers: Vec<(String, String)>,
    /// Refcounted body bytes straight off `wreq`. Stored as `Bytes` (not
    /// `Vec<u8>`) so construction is a cheap pointer bump instead of a full
    /// copy; the single unavoidable copy happens when `body()` hands the bytes
    /// to PHP, which owns its own strings. Empty for a streamed-to-disk
    /// response — there the body never enters PHP memory.
    body: Bytes,
    remote_addr: Option<String>,
    /// `Some(n)` when the body was streamed straight to a file (via the
    /// `sink` path): `n` is the number of bytes written. `None` for an ordinary
    /// in-memory response.
    downloaded_bytes: Option<u64>,
}

impl Response {
    /// Builds a response from data already extracted off the async runtime.
    pub(crate) fn new(
        status: u16,
        version: String,
        url: String,
        headers: Vec<(String, String)>,
        body: Bytes,
        remote_addr: Option<String>,
    ) -> Self {
        Self {
            status,
            version,
            url,
            headers,
            body,
            remote_addr,
            downloaded_bytes: None,
        }
    }

    /// Builds a response whose body was streamed to a file rather than read
    /// into memory. `downloaded_bytes` is the number of bytes written; `body()`
    /// returns an empty string for such a response.
    pub(crate) fn new_download(
        status: u16,
        version: String,
        url: String,
        headers: Vec<(String, String)>,
        remote_addr: Option<String>,
        downloaded_bytes: u64,
    ) -> Self {
        Self {
            status,
            version,
            url,
            headers,
            body: Bytes::new(),
            remote_addr,
            downloaded_bytes: Some(downloaded_bytes),
        }
    }
}

#[php_impl]
impl Response {
    /// HTTP status code (e.g. 200, 404).
    pub fn status(&self) -> u16 {
        self.status
    }

    /// HTTP protocol version string (e.g. `"HTTP/2.0"`).
    pub fn version(&self) -> &str {
        &self.version
    }

    /// Final URL after any redirects.
    pub fn url(&self) -> &str {
        &self.url
    }

    /// Raw response body as a binary-safe PHP string.
    pub fn body(&self) -> Zval {
        let mut zv = Zval::new();
        zv.set_zend_string(ZendStr::new(self.body.as_ref(), false));
        zv
    }

    /// All headers as a map of lowercased name => list of values. Names appear
    /// in the order they were first seen in the response, so `Set-Cookie`
    /// ordering and other arrival-sensitive headers stay observable from PHP.
    pub fn headers(&self) -> IndexMap<String, Vec<String>> {
        let mut map: IndexMap<String, Vec<String>> = IndexMap::new();
        for (name, value) in &self.headers {
            // Header names are ASCII per RFC 9110, so the byte-wise lowercase
            // is both correct and cheaper than the Unicode-aware `to_lowercase`.
            map.entry(name.to_ascii_lowercase())
                .or_default()
                .push(value.clone());
        }
        map
    }

    /// A single header by name (case-insensitive); multiple values are joined
    /// with `", "`. Returns `null` when the header is absent.
    pub fn header(&self, name: &str) -> Option<String> {
        // ASCII case-insensitive compare, no per-header allocation.
        let values: Vec<&str> = self
            .headers
            .iter()
            .filter(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
            .collect();
        if values.is_empty() {
            None
        } else {
            Some(values.join(", "))
        }
    }

    /// Remote peer address (`ip:port`) the response came from, if known.
    pub fn remote_addr(&self) -> Option<&str> {
        self.remote_addr.as_deref()
    }

    /// Number of bytes written to disk when the body was streamed via a sink,
    /// or `null` for an ordinary in-memory response. Lets the PHP layer report
    /// the download size without ever materializing the body as a string.
    pub fn downloaded_bytes(&self) -> Option<i64> {
        self.downloaded_bytes.map(|n| n as i64)
    }
}
