//! The reusable HTTP client — the heart of the library.
//!
//! One `Client` owns exactly one `wreq::Client`, which owns exactly one
//! connection pool. Reuse the same `Client` and keep-alive TCP/TLS connections
//! are reused deterministically; drop it (or call `close()`) and the pool and
//! all its sockets are torn down. There is no hidden global cache.
//!
//! Per-request building (headers, query, body) is done in the PHP layer, which
//! accumulates state immutably and then calls `request()` here. That keeps this
//! core minimal and the connection pool untouched by per-request tweaks.

use std::time::Duration;

use ext_php_rs::exception::PhpException;
use ext_php_rs::prelude::*;
use ext_php_rs::types::ZendHashTable;

use crate::convert::headers_from_table;
use crate::emulation::EmulationConfig;
use crate::error::map_wreq_error;
use crate::response::Response;
use crate::runtime::runtime;

/// Reusable HTTP client with a dedicated connection pool.
#[php_class]
#[php(name = "Wreq\\Ext\\Client")]
pub struct Client {
    /// `None` once `close()` has been called.
    inner: Option<wreq::Client>,
}

#[php_impl]
impl Client {
    /// Builds a client from an options array. See `build_wreq_client` for the
    /// supported keys.
    pub fn __construct(options: Option<&ZendHashTable>) -> PhpResult<Self> {
        let inner = build_wreq_client(options)?;
        Ok(Self { inner: Some(inner) })
    }

    /// Executes an HTTP request and returns the response with its body read.
    ///
    /// * `method`  — HTTP method (`GET`, `POST`, …).
    /// * `url`     — fully-formed URL (the PHP layer appends any query string).
    /// * `headers` — per-request headers (`name => value`).
    /// * `body`    — raw request body, already encoded by the PHP layer.
    pub fn request(
        &self,
        method: &str,
        url: &str,
        headers: Option<&ZendHashTable>,
        body: Option<String>,
    ) -> PhpResult<Response> {
        let mut builder = self.request_builder(method, url, headers)?;
        if let Some(body) = body {
            builder = builder.body(body);
        }
        self.execute(builder)
    }

    /// Executes a `multipart/form-data` request.
    ///
    /// * `fields` — text fields (`name => value`).
    /// * `files`  — a list of attachments; each is an array with `name`,
    ///   `contents` (raw bytes) and optional `filename` / `content_type`.
    pub fn request_multipart(
        &self,
        method: &str,
        url: &str,
        headers: Option<&ZendHashTable>,
        fields: Option<&ZendHashTable>,
        files: Option<&ZendHashTable>,
    ) -> PhpResult<Response> {
        let builder = self.request_builder(method, url, headers)?;
        let form = build_multipart(fields, files)?;
        self.execute(builder.multipart(form))
    }

    /// Releases the connection pool now, closing all idle keep-alive sockets.
    /// Subsequent requests through this client raise an exception.
    pub fn close(&mut self) {
        self.inner = None;
    }

    /// Whether the client is still usable (not yet closed).
    pub fn is_open(&self) -> bool {
        self.inner.is_some()
    }
}

impl Client {
    /// Starts a `RequestBuilder` with the method, URL and per-request headers.
    fn request_builder(
        &self,
        method: &str,
        url: &str,
        headers: Option<&ZendHashTable>,
    ) -> PhpResult<wreq::RequestBuilder> {
        let client = self.inner.as_ref().ok_or_else(|| {
            PhpException::default("client is closed: its connection pool has been released".into())
        })?;
        let http_method = wreq::Method::from_bytes(method.as_bytes())
            .map_err(|_| PhpException::default(format!("invalid HTTP method: '{method}'")))?;

        let mut builder = client.request(http_method, url);
        if let Some(table) = headers {
            builder = builder.headers(headers_from_table(table).map_err(PhpException::from)?);
        }
        Ok(builder)
    }

    /// Sends the request on the shared runtime and reads the full response.
    fn execute(&self, builder: wreq::RequestBuilder) -> PhpResult<Response> {
        runtime().block_on(async move {
            let resp = builder.send().await.map_err(map_wreq_error)?;

            let status = resp.status().as_u16();
            let version = format!("{:?}", resp.version());
            let url = resp.uri().to_string();
            let remote_addr = resp.remote_addr().map(|addr| addr.to_string());
            let headers = resp
                .headers()
                .iter()
                .map(|(name, value)| {
                    (
                        name.as_str().to_string(),
                        String::from_utf8_lossy(value.as_bytes()).into_owned(),
                    )
                })
                .collect();

            let body = resp.bytes().await.map_err(map_wreq_error)?.to_vec();

            Ok(Response::new(status, version, url, headers, body, remote_addr))
        })
    }
}

/// Builds a `multipart/form-data` form from PHP-side text fields and files.
fn build_multipart(
    fields: Option<&ZendHashTable>,
    files: Option<&ZendHashTable>,
) -> PhpResult<wreq::multipart::Form> {
    let mut form = wreq::multipart::Form::new();

    if let Some(fields) = fields {
        for (name, value) in fields.iter() {
            let name = name.to_string();
            let value = value
                .str()
                .ok_or_else(|| PhpException::default(format!("field '{name}' must be a string")))?;
            form = form.text(name, value.to_string());
        }
    }

    if let Some(files) = files {
        for (_, file) in files.iter() {
            let file = file
                .array()
                .ok_or_else(|| PhpException::default("each attachment must be an array".into()))?;

            let name = file
                .get("name")
                .and_then(|zv| zv.str())
                .ok_or_else(|| PhpException::default("attachment is missing 'name'".into()))?
                .to_string();

            // Raw bytes — read via the zend_string so binary file content
            // survives (a PHP string is not necessarily valid UTF-8).
            let contents = file
                .get("contents")
                .and_then(|zv| zv.zend_str())
                .ok_or_else(|| {
                    PhpException::default(format!("attachment '{name}' is missing 'contents'"))
                })?
                .as_ref()
                .to_vec();

            let mut part = wreq::multipart::Part::bytes(contents);
            if let Some(filename) = file.get("filename").and_then(|zv| zv.str()) {
                part = part.file_name(filename.to_string());
            }
            if let Some(mime) = file.get("content_type").and_then(|zv| zv.str()) {
                part = part
                    .mime_str(mime)
                    .map_err(|e| PhpException::default(format!("invalid content_type: {e}")))?;
            }
            form = form.part(name, part);
        }
    }

    Ok(form)
}

/// Builds a `wreq::Client` from a PHP options array.
///
/// Every `wreq::ClientBuilder` setting that can be expressed as a PHP scalar is
/// exposed. Settings that require Rust objects — custom `Identity`/`CertStore`,
/// `Http1Options`/`Http2Options`/`TlsOptions`, `KeyLog`, a custom DNS resolver,
/// tower layers, `orig_headers` and `cookie_provider` — are intentionally not
/// exposed; use the crate directly if you need those.
///
/// Recognized keys (all optional):
///
/// Emulation: `emulation`, `emulation_os`, `skip_http2`, `skip_headers`.
/// Identity/headers: `user_agent` (string), `headers` (map).
/// Connection pool: `pool_max_idle_per_host`, `pool_max_size` (int),
///   `pool_idle_timeout` (float seconds).
/// Timeouts (float seconds): `timeout`, `read_timeout`, `connect_timeout`.
/// Cookies: `cookies` (bool — per-client jar).
/// Compression (bool): `gzip`, `brotli`, `zstd`, `deflate`.
/// Redirects: `max_redirects` (int; 0 disables), `referer` (bool).
/// HTTP version: `http1_only`, `http2_only`, `https_only` (bool).
/// TCP: `tcp_nodelay`, `tcp_reuse_address` (bool); `tcp_keepalive`,
///   `tcp_keepalive_interval`, `tcp_user_timeout`, `tcp_happy_eyeballs_timeout`
///   (float seconds); `tcp_keepalive_retries`, `tcp_send_buffer_size`,
///   `tcp_recv_buffer_size` (int); `connection_verbose` (bool).
/// Network: `local_address` (IP string), `interface` (name, Unix only),
///   `proxy` (URL string), `no_proxy` (bool), `no_hickory_dns` (bool),
///   `resolve` (map host => "ip:port").
/// TLS: `verify` (bool — cert + hostname), `tls_sni` (bool), `tls_info` (bool),
///   `min_tls_version`, `max_tls_version` (string `"1.0"`..`"1.3"`).
fn build_wreq_client(options: Option<&ZendHashTable>) -> PhpResult<wreq::Client> {
    let mut builder = wreq::Client::builder();

    let Some(opts) = options else {
        return builder
            .build()
            .map_err(|e| PhpException::default(format!("failed to build HTTP client: {e}")));
    };

    // ---- emulation ----
    if let Some(emulation) = opt_str(opts, "emulation") {
        let os = opt_str(opts, "emulation_os");
        let skip_http2 = opt_bool(opts, "skip_http2").unwrap_or(false);
        let skip_headers = opt_bool(opts, "skip_headers").unwrap_or(false);
        let config = if os.is_some() || skip_http2 || skip_headers {
            EmulationConfig::detailed(emulation, os, skip_http2, skip_headers)
        } else {
            EmulationConfig::from_name(emulation)
        }
        .map_err(PhpException::default)?;
        builder = config.apply(builder);
    }

    // ---- identity / default headers ----
    if let Some(ua) = opt_str(opts, "user_agent") {
        builder = builder.user_agent(ua);
    }
    if let Some(table) = opts.get("headers").and_then(|zv| zv.array()) {
        builder = builder.default_headers(headers_from_table(table).map_err(PhpException::from)?);
    }

    // ---- connection pool ----
    if let Some(n) = opt_long(opts, "pool_max_idle_per_host") {
        builder = builder.pool_max_idle_per_host(checked_usize("pool_max_idle_per_host", n)?);
    }
    if let Some(n) = opt_long(opts, "pool_max_size") {
        builder = builder.pool_max_size(checked_u32("pool_max_size", n)?);
    }
    if let Some(secs) = opt_f64(opts, "pool_idle_timeout") {
        builder = builder.pool_idle_timeout(checked_duration("pool_idle_timeout", secs)?);
    }

    // ---- timeouts ----
    if let Some(secs) = opt_f64(opts, "timeout") {
        if secs > 0.0 {
            builder = builder.timeout(checked_duration("timeout", secs)?);
        }
    }
    if let Some(secs) = opt_f64(opts, "read_timeout") {
        if secs > 0.0 {
            builder = builder.read_timeout(checked_duration("read_timeout", secs)?);
        }
    }
    if let Some(secs) = opt_f64(opts, "connect_timeout") {
        if secs > 0.0 {
            builder = builder.connect_timeout(checked_duration("connect_timeout", secs)?);
        }
    }

    // ---- cookies ----
    if opt_bool(opts, "cookies").unwrap_or(false) {
        builder = builder.cookie_store(true);
    }

    // ---- compression ----
    if let Some(v) = opt_bool(opts, "gzip") {
        builder = builder.gzip(v);
    }
    if let Some(v) = opt_bool(opts, "brotli") {
        builder = builder.brotli(v);
    }
    if let Some(v) = opt_bool(opts, "zstd") {
        builder = builder.zstd(v);
    }
    if let Some(v) = opt_bool(opts, "deflate") {
        builder = builder.deflate(v);
    }

    // ---- redirects ----
    if let Some(max) = opt_long(opts, "max_redirects") {
        builder = builder.redirect(if max <= 0 {
            wreq::redirect::Policy::none()
        } else {
            wreq::redirect::Policy::limited(max as usize)
        });
    }
    if let Some(v) = opt_bool(opts, "referer") {
        builder = builder.referer(v);
    }

    // ---- HTTP version ----
    if opt_bool(opts, "http1_only").unwrap_or(false) {
        builder = builder.http1_only();
    }
    if opt_bool(opts, "http2_only").unwrap_or(false) {
        builder = builder.http2_only();
    }
    if let Some(v) = opt_bool(opts, "https_only") {
        builder = builder.https_only(v);
    }
    if let Some(v) = opt_bool(opts, "connection_verbose") {
        builder = builder.connection_verbose(v);
    }

    // ---- TCP ----
    if let Some(v) = opt_bool(opts, "tcp_nodelay") {
        builder = builder.tcp_nodelay(v);
    }
    if let Some(v) = opt_bool(opts, "tcp_reuse_address") {
        builder = builder.tcp_reuse_address(v);
    }
    if let Some(secs) = opt_f64(opts, "tcp_keepalive") {
        builder = builder.tcp_keepalive(checked_duration("tcp_keepalive", secs)?);
    }
    if let Some(secs) = opt_f64(opts, "tcp_keepalive_interval") {
        builder = builder.tcp_keepalive_interval(checked_duration("tcp_keepalive_interval", secs)?);
    }
    // `tcp_user_timeout` is a Linux-family socket option (TCP_USER_TIMEOUT).
    #[cfg(any(target_os = "linux", target_os = "android", target_os = "fuchsia"))]
    if let Some(secs) = opt_f64(opts, "tcp_user_timeout") {
        builder = builder.tcp_user_timeout(checked_duration("tcp_user_timeout", secs)?);
    }
    if let Some(secs) = opt_f64(opts, "tcp_happy_eyeballs_timeout") {
        builder =
            builder.tcp_happy_eyeballs_timeout(checked_duration("tcp_happy_eyeballs_timeout", secs)?);
    }
    if let Some(n) = opt_long(opts, "tcp_keepalive_retries") {
        builder = builder.tcp_keepalive_retries(checked_u32("tcp_keepalive_retries", n)?);
    }
    if let Some(n) = opt_long(opts, "tcp_send_buffer_size") {
        builder = builder.tcp_send_buffer_size(checked_usize("tcp_send_buffer_size", n)?);
    }
    if let Some(n) = opt_long(opts, "tcp_recv_buffer_size") {
        builder = builder.tcp_recv_buffer_size(checked_usize("tcp_recv_buffer_size", n)?);
    }

    // ---- network ----
    if let Some(addr) = opt_str(opts, "local_address") {
        let ip: std::net::IpAddr = addr
            .parse()
            .map_err(|_| PhpException::default(format!("invalid local_address IP: '{addr}'")))?;
        builder = builder.local_address(ip);
    }
    #[cfg(unix)]
    if let Some(iface) = opt_str(opts, "interface") {
        builder = builder.interface(iface.to_string());
    }
    if let Some(url) = opt_str(opts, "proxy") {
        let proxy = wreq::Proxy::all(url)
            .map_err(|e| PhpException::default(format!("invalid proxy '{url}': {e}")))?;
        builder = builder.proxy(proxy);
    }
    if opt_bool(opts, "no_proxy").unwrap_or(false) {
        builder = builder.no_proxy();
    }
    if opt_bool(opts, "no_hickory_dns").unwrap_or(false) {
        builder = builder.no_hickory_dns();
    }
    if let Some(table) = opts.get("resolve").and_then(|zv| zv.array()) {
        for (host, value) in table.iter() {
            let host = host.to_string();
            let target = value
                .str()
                .ok_or_else(|| PhpException::default(format!("resolve['{host}'] must be a string")))?;
            let addr: std::net::SocketAddr = target.parse().map_err(|_| {
                PhpException::default(format!("resolve['{host}'] must be 'ip:port', got '{target}'"))
            })?;
            builder = builder.resolve(host, addr);
        }
    }

    // ---- TLS ----
    if let Some(verify) = opt_bool(opts, "verify") {
        builder = builder.cert_verification(verify).verify_hostname(verify);
    }
    if let Some(v) = opt_bool(opts, "tls_sni") {
        builder = builder.tls_sni(v);
    }
    if let Some(v) = opt_bool(opts, "tls_info") {
        builder = builder.tls_info(v);
    }
    if let Some(v) = opt_str(opts, "min_tls_version") {
        builder = builder.min_tls_version(parse_tls_version(v).map_err(PhpException::default)?);
    }
    if let Some(v) = opt_str(opts, "max_tls_version") {
        builder = builder.max_tls_version(parse_tls_version(v).map_err(PhpException::default)?);
    }

    builder
        .build()
        .map_err(|e| PhpException::default(format!("failed to build HTTP client: {e}")))
}

/// Reads a numeric Zval as `f64`, accepting both PHP floats and integers.
fn double_value(zv: &ext_php_rs::types::Zval) -> Option<f64> {
    zv.double().or_else(|| zv.long().map(|n| n as f64))
}

/// Reads an option as a bool.
fn opt_bool(opts: &ZendHashTable, key: &str) -> Option<bool> {
    opts.get(key).and_then(|zv| zv.bool())
}

/// Reads an option as an integer.
fn opt_long(opts: &ZendHashTable, key: &str) -> Option<i64> {
    opts.get(key).and_then(|zv| zv.long())
}

/// Reads an option as a string slice.
fn opt_str<'a>(opts: &'a ZendHashTable, key: &str) -> Option<&'a str> {
    opts.get(key).and_then(|zv| zv.str())
}

/// Reads an option as a float (accepts PHP ints too).
fn opt_f64(opts: &ZendHashTable, key: &str) -> Option<f64> {
    opts.get(key).and_then(double_value)
}

/// Converts a PHP integer option into a `u32`, rejecting negatives and values
/// that would wrap, instead of silently truncating with `as u32`.
fn checked_u32(option: &str, n: i64) -> PhpResult<u32> {
    u32::try_from(n).map_err(|_| {
        PhpException::default(format!(
            "option '{option}' must be an integer between 0 and {}",
            u32::MAX
        ))
    })
}

/// Converts a PHP integer option into a `usize`, rejecting negatives (and, on
/// 32-bit targets, values that would not fit).
fn checked_usize(option: &str, n: i64) -> PhpResult<usize> {
    usize::try_from(n).map_err(|_| {
        PhpException::default(format!(
            "option '{option}' must be a non-negative integer that fits in usize"
        ))
    })
}

/// Parses a `"1.0".."1.3"` string into a `wreq` TLS version.
fn parse_tls_version(value: &str) -> Result<wreq::tls::TlsVersion, String> {
    use wreq::tls::TlsVersion;
    match value.trim() {
        "1.0" | "1" | "10" => Ok(TlsVersion::TLS_1_0),
        "1.1" | "11" => Ok(TlsVersion::TLS_1_1),
        "1.2" | "12" => Ok(TlsVersion::TLS_1_2),
        "1.3" | "13" => Ok(TlsVersion::TLS_1_3),
        other => Err(format!(
            "invalid TLS version '{other}' (expected '1.0', '1.1', '1.2' or '1.3')"
        )),
    }
}

/// Converts a seconds value from PHP into a `Duration`, rejecting NaN, infinity,
/// negatives and absurdly large values instead of letting `Duration::from_secs_f64`
/// panic inside the extension.
fn checked_duration(option: &str, secs: f64) -> PhpResult<Duration> {
    // One year — any timeout/keep-alive beyond this is certainly a mistake.
    const MAX_SECS: f64 = 31_536_000.0;

    if !secs.is_finite() || secs < 0.0 {
        return Err(PhpException::default(format!(
            "option '{option}' must be a finite, non-negative number of seconds"
        )));
    }
    if secs > MAX_SECS {
        return Err(PhpException::default(format!(
            "option '{option}' is unreasonably large ({secs} seconds)"
        )));
    }
    Ok(Duration::from_secs_f64(secs))
}
