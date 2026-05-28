//! Raw HTTP response exposed to PHP.
//!
//! This is deliberately thin: it exposes status, headers and the body as raw
//! bytes. All the Laravel-style sugar (`json()`, `object()`, `successful()`,
//! `resource()`, …) lives in the PHP layer, which wraps this object.
//!
//! The body is read eagerly when the response is constructed, so every getter
//! is idempotent and can be called any number of times.

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
    body: Vec<u8>,
    remote_addr: Option<String>,
}

impl Response {
    /// Builds a response from data already extracted off the async runtime.
    pub(crate) fn new(
        status: u16,
        version: String,
        url: String,
        headers: Vec<(String, String)>,
        body: Vec<u8>,
        remote_addr: Option<String>,
    ) -> Self {
        Self {
            status,
            version,
            url,
            headers,
            body,
            remote_addr,
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
    pub fn version(&self) -> String {
        self.version.clone()
    }

    /// Final URL after any redirects.
    pub fn url(&self) -> String {
        self.url.clone()
    }

    /// Raw response body as a binary-safe PHP string.
    pub fn body(&self) -> Zval {
        let mut zv = Zval::new();
        zv.set_zend_string(ZendStr::new(&self.body, false));
        zv
    }

    /// All headers as a map of lowercased name => list of values. Names appear
    /// in the order they were first seen in the response, so `Set-Cookie`
    /// ordering and other arrival-sensitive headers stay observable from PHP.
    pub fn headers(&self) -> IndexMap<String, Vec<String>> {
        let mut map: IndexMap<String, Vec<String>> = IndexMap::new();
        for (name, value) in &self.headers {
            map.entry(name.to_lowercase())
                .or_default()
                .push(value.clone());
        }
        map
    }

    /// A single header by name (case-insensitive); multiple values are joined
    /// with `", "`. Returns `null` when the header is absent.
    pub fn header(&self, name: &str) -> Option<String> {
        let needle = name.to_lowercase();
        let values: Vec<&str> = self
            .headers
            .iter()
            .filter(|(k, _)| k.to_lowercase() == needle)
            .map(|(_, v)| v.as_str())
            .collect();
        if values.is_empty() {
            None
        } else {
            Some(values.join(", "))
        }
    }

    /// Remote peer address (`ip:port`) the response came from, if known.
    pub fn remote_addr(&self) -> Option<String> {
        self.remote_addr.clone()
    }
}
