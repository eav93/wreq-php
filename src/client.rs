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
use ext_php_rs::types::{ZendHashTable, Zval};

use crate::convert::headers_from_table;
use crate::emulation::EmulationConfig;
use crate::error::map_wreq_error;
use crate::response::Response;
use crate::runtime;

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
    /// * `body`    — raw request body as a PHP string. Read via `zend_str()`
    ///   so non-UTF-8 bytes (protobuf, msgpack, raw files, etc.) survive
    ///   intact; taking it as a Rust `String` would refuse anything but valid
    ///   UTF-8 because PHP strings are byte arrays, not Unicode.
    pub fn request(
        &self,
        method: &str,
        url: &str,
        headers: Option<&ZendHashTable>,
        body: Option<&Zval>,
    ) -> PhpResult<Response> {
        let builder = apply_body(self.request_builder(method, url, headers)?, body)?;
        self.execute(builder)
    }

    /// Executes a request and streams the response body straight to `path`,
    /// never materializing it as a PHP string. Built for large downloads: the
    /// body is written to disk chunk by chunk, so memory stays flat regardless
    /// of how big the response is. The returned `Response` carries status and
    /// headers but an empty body; `downloaded_bytes()` reports how much was
    /// written.
    pub fn request_to_file(
        &self,
        method: &str,
        url: &str,
        headers: Option<&ZendHashTable>,
        body: Option<&Zval>,
        path: &str,
    ) -> PhpResult<Response> {
        let builder = apply_body(self.request_builder(method, url, headers)?, body)?;
        self.execute_to_file(builder, path)
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

    /// The release version the native extension was built from (e.g. `0.1.9`),
    /// or `0.0.0-dev` for a local build. The PHP layer compares it with the
    /// Composer package version to detect a binary/wrapper mismatch.
    pub fn extension_version() -> &'static str {
        env!("WREQ_PHP_BUILD_VERSION")
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
        runtime::block_on(async move {
            let resp = builder.send().await.map_err(map_wreq_error)?;
            let meta = ResponseMeta::extract(&resp);

            // Keep the refcounted `Bytes` as-is; the only copy into PHP-owned
            // memory happens later, in `Response::body()`.
            let body = resp.bytes().await.map_err(map_wreq_error)?;

            Ok(meta.into_response(body))
        })
    }

    /// Sends the request and streams the body to `path` chunk by chunk, so peak
    /// memory stays flat no matter how large the response is.
    fn execute_to_file(&self, builder: wreq::RequestBuilder, path: &str) -> PhpResult<Response> {
        use std::io::Write;

        runtime::block_on(async move {
            let mut resp = builder.send().await.map_err(map_wreq_error)?;
            let meta = ResponseMeta::extract(&resp);

            let file = std::fs::File::create(path).map_err(|e| {
                PhpException::default(format!("failed to open download sink '{path}': {e}"))
            })?;
            // Buffer the writes: chunks arrive in network-sized pieces, and
            // batching them into larger syscalls keeps large downloads fast.
            let mut writer = std::io::BufWriter::new(file);
            let mut total: u64 = 0;

            // `chunk()` yields the body incrementally; nothing larger than one
            // network chunk is ever held in memory at once.
            while let Some(chunk) = resp.chunk().await.map_err(map_wreq_error)? {
                writer.write_all(&chunk).map_err(|e| {
                    PhpException::default(format!("failed writing to download sink '{path}': {e}"))
                })?;
                total += chunk.len() as u64;
            }
            writer.flush().map_err(|e| {
                PhpException::default(format!("failed flushing download sink '{path}': {e}"))
            })?;

            Ok(meta.into_download(total))
        })
    }
}

/// Response metadata extracted off the wire before the body is consumed.
///
/// Both `execute` (in-memory) and `execute_to_file` (streamed) pull the same
/// status/headers/address up front, then diverge on how they handle the body.
struct ResponseMeta {
    status: u16,
    version: String,
    url: String,
    remote_addr: Option<String>,
    headers: Vec<(String, String)>,
}

impl ResponseMeta {
    fn extract(resp: &wreq::Response) -> Self {
        Self {
            status: resp.status().as_u16(),
            version: format!("{:?}", resp.version()),
            url: resp.uri().to_string(),
            remote_addr: resp.remote_addr().map(|addr| addr.to_string()),
            headers: resp
                .headers()
                .iter()
                .map(|(name, value)| {
                    (
                        name.as_str().to_string(),
                        String::from_utf8_lossy(value.as_bytes()).into_owned(),
                    )
                })
                .collect(),
        }
    }

    fn into_response(self, body: bytes::Bytes) -> Response {
        Response::new(
            self.status,
            self.version,
            self.url,
            self.headers,
            body,
            self.remote_addr,
        )
    }

    fn into_download(self, downloaded_bytes: u64) -> Response {
        Response::new_download(
            self.status,
            self.version,
            self.url,
            self.headers,
            self.remote_addr,
            downloaded_bytes,
        )
    }
}

/// Applies an optional PHP request body to the builder.
///
/// `Option<&Zval>` matches PHP `null` as `Some(null_zval)` (not `None`) because
/// `&Zval` has no type restriction, so a missing body is distinguished by an
/// explicit null check. Read via `zend_str()` so non-UTF-8 bytes survive.
fn apply_body(
    builder: wreq::RequestBuilder,
    body: Option<&Zval>,
) -> PhpResult<wreq::RequestBuilder> {
    if let Some(zv) = body {
        if !zv.is_null() {
            let bytes = zv
                .zend_str()
                .ok_or_else(|| PhpException::default("body must be a string or null".into()))?
                .as_bytes()
                .to_vec();
            return Ok(builder.body(bytes));
        }
    }
    Ok(builder)
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
///   `proxy` (URL string — `http://`, `https://`, `socks4://`, `socks4a://`,
///   `socks5://`, `socks5h://`), `no_proxy` (bool), `no_hickory_dns` (bool),
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

    validate_options(opts)?;

    // ---- emulation ----
    // Two surface forms, both documented in the README:
    //   'emulation' => 'chrome_131'                              // flat
    //   'emulation' => ['profile' => 'chrome_131', 'os' => 'windows']  // nested
    // The nested form lets callers package the random/like helpers into one
    // value without scattering `emulation_os` next to it. A nested `os` wins
    // over a top-level `emulation_os`.
    if let Some(zv) = opts.get("emulation") {
        let (profile, nested_os) = if let Some(inner) = zv.array() {
            let profile = inner.get("profile").and_then(|z| z.str()).ok_or_else(|| {
                PhpException::default("emulation['profile'] must be a string".into())
            })?;
            let os = inner.get("os").and_then(|z| z.str());
            (profile, os)
        } else if let Some(name) = zv.str() {
            (name, None)
        } else {
            return Err(PhpException::default(
                "option 'emulation' must be a string or an array".into(),
            ));
        };

        let os = nested_os.or_else(|| opt_str(opts, "emulation_os"));
        let skip_http2 = opt_bool(opts, "skip_http2").unwrap_or(false);
        let skip_headers = opt_bool(opts, "skip_headers").unwrap_or(false);
        let config = if os.is_some() || skip_http2 || skip_headers {
            EmulationConfig::detailed(profile, os, skip_http2, skip_headers)
        } else {
            EmulationConfig::from_name(profile)
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
        builder = builder.pool_max_idle_per_host(
            checked_usize("pool_max_idle_per_host", n).map_err(PhpException::default)?,
        );
    }
    if let Some(n) = opt_long(opts, "pool_max_size") {
        builder =
            builder.pool_max_size(checked_u32("pool_max_size", n).map_err(PhpException::default)?);
    }
    if let Some(secs) = opt_f64(opts, "pool_idle_timeout") {
        builder = builder.pool_idle_timeout(
            checked_duration("pool_idle_timeout", secs).map_err(PhpException::default)?,
        );
    }

    // ---- timeouts ----
    if let Some(secs) = opt_f64(opts, "timeout") {
        if secs > 0.0 {
            builder =
                builder.timeout(checked_duration("timeout", secs).map_err(PhpException::default)?);
        }
    }
    if let Some(secs) = opt_f64(opts, "read_timeout") {
        if secs > 0.0 {
            builder = builder.read_timeout(
                checked_duration("read_timeout", secs).map_err(PhpException::default)?,
            );
        }
    }
    if let Some(secs) = opt_f64(opts, "connect_timeout") {
        if secs > 0.0 {
            builder = builder.connect_timeout(
                checked_duration("connect_timeout", secs).map_err(PhpException::default)?,
            );
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
        builder = builder
            .tcp_keepalive(checked_duration("tcp_keepalive", secs).map_err(PhpException::default)?);
    }
    if let Some(secs) = opt_f64(opts, "tcp_keepalive_interval") {
        builder = builder.tcp_keepalive_interval(
            checked_duration("tcp_keepalive_interval", secs).map_err(PhpException::default)?,
        );
    }
    // `tcp_user_timeout` is a Linux-family socket option (TCP_USER_TIMEOUT).
    #[cfg(any(target_os = "linux", target_os = "android", target_os = "fuchsia"))]
    if let Some(secs) = opt_f64(opts, "tcp_user_timeout") {
        builder = builder.tcp_user_timeout(
            checked_duration("tcp_user_timeout", secs).map_err(PhpException::default)?,
        );
    }
    if let Some(secs) = opt_f64(opts, "tcp_happy_eyeballs_timeout") {
        builder = builder.tcp_happy_eyeballs_timeout(
            checked_duration("tcp_happy_eyeballs_timeout", secs).map_err(PhpException::default)?,
        );
    }
    if let Some(n) = opt_long(opts, "tcp_keepalive_retries") {
        builder = builder.tcp_keepalive_retries(
            checked_u32("tcp_keepalive_retries", n).map_err(PhpException::default)?,
        );
    }
    if let Some(n) = opt_long(opts, "tcp_send_buffer_size") {
        builder = builder.tcp_send_buffer_size(
            checked_usize("tcp_send_buffer_size", n).map_err(PhpException::default)?,
        );
    }
    if let Some(n) = opt_long(opts, "tcp_recv_buffer_size") {
        builder = builder.tcp_recv_buffer_size(
            checked_usize("tcp_recv_buffer_size", n).map_err(PhpException::default)?,
        );
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
            let target = value.str().ok_or_else(|| {
                PhpException::default(format!("resolve['{host}'] must be a string"))
            })?;
            let addr: std::net::SocketAddr = target.parse().map_err(|_| {
                PhpException::default(format!(
                    "resolve['{host}'] must be 'ip:port', got '{target}'"
                ))
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

/// Every option key `build_wreq_client` knows. Used to reject typos before
/// they silently no-op. `base_url` is consumed by the PHP layer (`Wreq\Client`)
/// and passes through here untouched, so we accept it without applying it.
const KNOWN_OPTIONS: &[&str] = &[
    "emulation",
    "emulation_os",
    "skip_http2",
    "skip_headers",
    "user_agent",
    "headers",
    "base_url",
    "pool_max_idle_per_host",
    "pool_max_size",
    "pool_idle_timeout",
    "timeout",
    "read_timeout",
    "connect_timeout",
    "cookies",
    "gzip",
    "brotli",
    "zstd",
    "deflate",
    "max_redirects",
    "referer",
    "http1_only",
    "http2_only",
    "https_only",
    "connection_verbose",
    "tcp_nodelay",
    "tcp_reuse_address",
    "tcp_keepalive",
    "tcp_keepalive_interval",
    "tcp_user_timeout",
    "tcp_happy_eyeballs_timeout",
    "tcp_keepalive_retries",
    "tcp_send_buffer_size",
    "tcp_recv_buffer_size",
    "local_address",
    "interface",
    "proxy",
    "no_proxy",
    "no_hickory_dns",
    "resolve",
    "verify",
    "tls_sni",
    "tls_info",
    "min_tls_version",
    "max_tls_version",
];

/// Rejects unknown option keys and mutually exclusive combinations *before*
/// `build_wreq_client` walks the table — silently ignoring a typo like
/// `pool_max_idel_per_host` used to be a tedious-to-debug footgun.
fn validate_options(opts: &ZendHashTable) -> PhpResult<()> {
    for (key, _) in opts.iter() {
        let name = key.to_string();
        if !KNOWN_OPTIONS.contains(&name.as_str()) {
            let msg = match closest_option(&name) {
                Some(suggestion) => {
                    format!("unknown client option '{name}'; did you mean '{suggestion}'?")
                }
                None => format!("unknown client option '{name}'"),
            };
            return Err(PhpException::default(msg));
        }
    }

    if opt_bool(opts, "http1_only").unwrap_or(false)
        && opt_bool(opts, "http2_only").unwrap_or(false)
    {
        return Err(PhpException::default(
            "options 'http1_only' and 'http2_only' are mutually exclusive".into(),
        ));
    }

    if opts.get("proxy").is_some() && opt_bool(opts, "no_proxy").unwrap_or(false) {
        return Err(PhpException::default(
            "options 'proxy' and 'no_proxy' are mutually exclusive".into(),
        ));
    }

    Ok(())
}

/// Suggests the closest known option name if the input looks like a typo.
/// Threshold of 3 edits keeps wrong-but-related names (`timeout` vs
/// `proxy`) out of the suggestion while still catching common slips like
/// `pool_max_idel_per_host` (1 edit from `pool_max_idle_per_host`).
fn closest_option(input: &str) -> Option<&'static str> {
    KNOWN_OPTIONS
        .iter()
        .map(|opt| (*opt, edit_distance(input, opt)))
        .filter(|(_, d)| *d <= 3)
        .min_by_key(|(_, d)| *d)
        .map(|(opt, _)| opt)
}

/// Standard Levenshtein distance, two-row rolling buffer.
fn edit_distance(a: &str, b: &str) -> usize {
    let a = a.as_bytes();
    let b = b.as_bytes();
    if a.is_empty() {
        return b.len();
    }
    if b.is_empty() {
        return a.len();
    }
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut curr = vec![0usize; b.len() + 1];
    for i in 1..=a.len() {
        curr[0] = i;
        for j in 1..=b.len() {
            let cost = if a[i - 1] == b[j - 1] { 0 } else { 1 };
            curr[j] = (curr[j - 1] + 1).min(prev[j] + 1).min(prev[j - 1] + cost);
        }
        std::mem::swap(&mut prev, &mut curr);
    }
    prev[b.len()]
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
/// that would wrap, instead of silently truncating with `as u32`. Returns a
/// plain `String` so the function stays ext-php-rs-free and unit-testable
/// without dragging in PHP runtime symbols.
fn checked_u32(option: &str, n: i64) -> std::result::Result<u32, String> {
    u32::try_from(n).map_err(|_| {
        format!(
            "option '{option}' must be an integer between 0 and {}",
            u32::MAX
        )
    })
}

/// Converts a PHP integer option into a `usize`, rejecting negatives (and, on
/// 32-bit targets, values that would not fit).
fn checked_usize(option: &str, n: i64) -> std::result::Result<usize, String> {
    usize::try_from(n)
        .map_err(|_| format!("option '{option}' must be a non-negative integer that fits in usize"))
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
/// panic inside the extension. Returns a plain `String` for unit-test friendliness.
fn checked_duration(option: &str, secs: f64) -> std::result::Result<Duration, String> {
    // One year — any timeout/keep-alive beyond this is certainly a mistake.
    const MAX_SECS: f64 = 31_536_000.0;

    if !secs.is_finite() || secs < 0.0 {
        return Err(format!(
            "option '{option}' must be a finite, non-negative number of seconds"
        ));
    }
    if secs > MAX_SECS {
        return Err(format!(
            "option '{option}' is unreasonably large ({secs} seconds)"
        ));
    }
    Ok(Duration::from_secs_f64(secs))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_tls_version_accepts_canonical_strings() {
        for input in ["1.0", "1.1", "1.2", "1.3"] {
            assert!(parse_tls_version(input).is_ok(), "{input} should parse");
        }
    }

    #[test]
    fn parse_tls_version_accepts_terse_forms() {
        assert!(parse_tls_version("12").is_ok());
        assert!(parse_tls_version("13").is_ok());
        assert!(parse_tls_version(" 1.2 ").is_ok());
    }

    #[test]
    fn parse_tls_version_rejects_unknown() {
        assert!(parse_tls_version("1.4").is_err());
        assert!(parse_tls_version("ssl3").is_err());
    }

    #[test]
    fn checked_u32_accepts_in_range() {
        assert_eq!(checked_u32("k", 0).unwrap(), 0);
        assert_eq!(checked_u32("k", 42).unwrap(), 42);
        assert_eq!(checked_u32("k", u32::MAX as i64).unwrap(), u32::MAX);
    }

    #[test]
    fn checked_u32_rejects_negatives_and_overflow() {
        assert!(checked_u32("k", -1).is_err());
        assert!(checked_u32("k", (u32::MAX as i64) + 1).is_err());
    }

    #[test]
    fn checked_usize_rejects_negatives() {
        assert!(checked_usize("k", -1).is_err());
        assert_eq!(checked_usize("k", 7).unwrap(), 7);
    }

    #[test]
    fn checked_duration_rejects_nan_and_infinity() {
        assert!(checked_duration("k", f64::NAN).is_err());
        assert!(checked_duration("k", f64::INFINITY).is_err());
        assert!(checked_duration("k", f64::NEG_INFINITY).is_err());
    }

    #[test]
    fn checked_duration_rejects_negatives_and_silly_large() {
        assert!(checked_duration("k", -0.001).is_err());
        // > 1 year, should be rejected as unreasonable
        assert!(checked_duration("k", 60.0 * 60.0 * 24.0 * 366.0).is_err());
    }

    #[test]
    fn checked_duration_accepts_typical_values() {
        let d = checked_duration("k", 1.5).unwrap();
        assert_eq!(d, Duration::from_millis(1_500));
    }

    #[test]
    fn edit_distance_basic() {
        assert_eq!(edit_distance("kitten", "kitten"), 0);
        assert_eq!(edit_distance("kitten", "sitten"), 1);
        assert_eq!(edit_distance("kitten", "sitting"), 3);
        assert_eq!(edit_distance("", "abc"), 3);
        assert_eq!(edit_distance("abc", ""), 3);
    }

    #[test]
    fn closest_option_suggests_for_typo() {
        // 1-edit typo
        assert_eq!(
            closest_option("pool_max_idel_per_host"),
            Some("pool_max_idle_per_host"),
        );
        // 2-edit typo
        assert_eq!(closest_option("timout"), Some("timeout"));
    }

    #[test]
    fn closest_option_silent_when_far() {
        assert_eq!(closest_option("completely_unrelated_name"), None);
    }
}
