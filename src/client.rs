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

use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use ext_php_rs::exception::PhpException;
use ext_php_rs::prelude::*;
use ext_php_rs::types::{ZendHashTable, Zval};

use crate::convert::headers_from_table;
use crate::emulation::EmulationConfig;
use crate::error::{map_wreq_error, ForkException};
use crate::response::Response;
use crate::runtime;

/// PID of the process that first issued a request through a Hickory-backed
/// client (0 = none yet). Set lazily on first use — never in module init — so
/// php-fpm workers (themselves forks of the master) are not misflagged: a
/// worker becomes the owner when *it* first uses Hickory. A later `fork(2)`
/// inside that worker changes the PID and trips the fork guard. See the
/// module-level note in `runtime` and [`Client::fork_guard`].
static HICKORY_OWNER: AtomicU32 = AtomicU32::new(0);

/// Reusable HTTP client with a dedicated connection pool.
#[php_class]
#[php(name = "Wreq\\Ext\\Client")]
pub struct Client {
    /// `None` once `close()` has been called.
    inner: Option<wreq::Client>,
    /// Whether this client resolves via Hickory (i.e. `no_hickory_dns` was not
    /// set). Hickory is not fork-safe; the system resolver (getaddrinfo) is.
    uses_hickory: bool,
    /// Optional cap on streamed download size (`request_to_file`); `None` = no
    /// limit. Guards against a slow server filling the disk drip by drip.
    max_download_bytes: Option<u64>,
}

#[php_impl]
impl Client {
    /// Builds a client from an options array. See `build_wreq_client` for the
    /// supported keys.
    pub fn __construct(options: Option<&ZendHashTable>) -> PhpResult<Self> {
        // Hickory is the default resolver; `no_hickory_dns => true` opts into
        // the fork-safe system resolver. Read it before consuming `options`.
        let (uses_hickory, max_download_bytes) = match options {
            Some(o) => {
                let uses_hickory = !opt_bool(o, "no_hickory_dns")?.unwrap_or(false);
                let max = opt_long(o, "max_download_bytes")?
                    .map(|n| {
                        u64::try_from(n).map_err(|_| {
                            opt_type_error("max_download_bytes", "a non-negative integer")
                        })
                    })
                    .transpose()?;
                (uses_hickory, max)
            }
            None => (true, None),
        };
        let inner = build_wreq_client(options)?;
        Ok(Self {
            inner: Some(inner),
            uses_hickory,
            max_download_bytes,
        })
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
    // `path` is `Option` (with a runtime check) so it is not a *required*
    // parameter after the optional `headers`/`body` — that shape is deprecated
    // in PHP 8.5 and would emit warnings at registration.
    pub fn request_to_file(
        &self,
        method: &str,
        url: &str,
        headers: Option<&ZendHashTable>,
        body: Option<&Zval>,
        path: Option<&str>,
    ) -> PhpResult<Response> {
        let path = path
            .ok_or_else(|| PhpException::default("request_to_file: 'path' is required".into()))?;
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
    /// Rejects use of Hickory DNS after `fork(2)` once the parent has used it.
    ///
    /// Hickory's resolver is a **process-global** singleton bound to the Tokio
    /// runtime that first built it. That runtime does not survive `fork`, so
    /// any lookup in a forked child hangs until the connect timeout — and this
    /// is global, so building a *fresh* client in the child does not help: it
    /// still shares the parent's dead resolver. The only escape once the parent
    /// has touched Hickory is `no_hickory_dns` (the system resolver is
    /// fork-safe).
    ///
    /// We tag the owning PID lazily on first Hickory use (never at module init,
    /// so php-fpm workers are not misflagged). A PID mismatch means we inherited
    /// a live Hickory resolver across `fork`; we raise `ForkException` instead
    /// of hanging. If the parent never used Hickory (owner still 0), the child
    /// claims ownership and builds the resolver fresh on its own runtime — that
    /// case is allowed.
    ///
    /// Clients with `no_hickory_dns` use getaddrinfo and skip this check.
    fn fork_guard(&self) -> PhpResult<()> {
        if !self.uses_hickory {
            return Ok(());
        }
        let pid = std::process::id();
        // AcqRel on success / Acquire on failure is sufficient here — we only
        // need the single-location read-modify-write to be atomic; there is no
        // other shared state to order against. Once set, the owner is never
        // reset: the Hickory resolver is process-global and never torn down, so
        // the owning PID stays valid for the life of the process.
        match HICKORY_OWNER.compare_exchange(0, pid, Ordering::AcqRel, Ordering::Acquire) {
            // First Hickory use in this process — we own the resolver.
            Ok(_) => Ok(()),
            // Same process re-using its own resolver.
            Err(owner) if owner == pid => Ok(()),
            // Different PID: the parent already built the process-global Hickory
            // resolver, which is now dead in this child. A fresh client would
            // inherit the same dead singleton, so the only fix is system DNS.
            Err(owner) => Err(PhpException::from_class::<ForkException>(format!(
                "Hickory DNS resolver inherited across fork(2): initialised in PID {owner}, \
                 used in PID {pid}. Hickory's resolver is process-global and bound to the \
                 parent's runtime, so a new client in the child cannot recover it. Set \
                 no_hickory_dns => true (the system resolver is fork-safe)."
            ))),
        }
    }

    /// Starts a `RequestBuilder` with the method, URL and per-request headers.
    fn request_builder(
        &self,
        method: &str,
        url: &str,
        headers: Option<&ZendHashTable>,
    ) -> PhpResult<wreq::RequestBuilder> {
        // Report a closed client before the fork guard, so a closed client used
        // in a forked child says "closed" rather than raising ForkException.
        let client = self.inner.as_ref().ok_or_else(|| {
            PhpException::default("client is closed: its connection pool has been released".into())
        })?;
        self.fork_guard()?;
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
    ///
    /// The body is written to a sibling `<path>.part` file and atomically
    /// renamed onto `path` only after a clean flush — a network/disk error (or
    /// exceeding `max_download_bytes`) removes the partial file and leaves any
    /// existing `path` untouched, so callers never see a truncated download.
    fn execute_to_file(&self, builder: wreq::RequestBuilder, path: &str) -> PhpResult<Response> {
        use futures_util::StreamExt;
        use std::io::Write;

        let max = self.max_download_bytes;
        runtime::block_on(async move {
            let resp = builder.send().await.map_err(map_wreq_error)?;
            let meta = ResponseMeta::extract(&resp);

            let tmp = format!("{path}.part");
            let file = std::fs::File::create(&tmp).map_err(|e| {
                PhpException::default(format!("failed to open download sink '{tmp}': {e}"))
            })?;
            // Buffer the writes: chunks arrive in network-sized pieces, and
            // batching them into larger syscalls keeps large downloads fast.
            let mut writer = std::io::BufWriter::new(file);
            let mut total: u64 = 0;

            // Stream to the temp file; on any error, drop the writer and unlink
            // the partial file before returning.
            let streamed: PhpResult<()> = async {
                // `bytes_stream()` yields the body incrementally; nothing larger
                // than one network chunk is ever held in memory at once.
                let mut stream = resp.bytes_stream();
                while let Some(chunk) = stream.next().await {
                    let chunk = chunk.map_err(map_wreq_error)?;
                    total += chunk.len() as u64;
                    if let Some(limit) = max {
                        if total > limit {
                            return Err(PhpException::default(format!(
                                "download exceeds max_download_bytes ({limit})"
                            )));
                        }
                    }
                    writer.write_all(&chunk).map_err(|e| {
                        PhpException::default(format!(
                            "failed writing to download sink '{tmp}': {e}"
                        ))
                    })?;
                }
                writer.flush().map_err(|e| {
                    PhpException::default(format!("failed flushing download sink '{tmp}': {e}"))
                })?;
                Ok(())
            }
            .await;

            // Close the file (drop the writer) before renaming/removing.
            drop(writer);

            match streamed {
                Ok(()) => {
                    std::fs::rename(&tmp, path).map_err(|e| {
                        let _ = std::fs::remove_file(&tmp);
                        PhpException::default(format!(
                            "failed to finalize download '{path}' (renaming '{tmp}'): {e}"
                        ))
                    })?;
                    Ok(meta.into_download(total))
                }
                Err(e) => {
                    let _ = std::fs::remove_file(&tmp);
                    Err(e)
                }
            }
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
            // PHP forms routinely carry ints/floats/bools; coerce scalars to
            // string (as PHP's own `(string)` cast would) instead of rejecting.
            let value = scalar_to_string(value).ok_or_else(|| {
                PhpException::default(format!(
                    "field '{name}' must be a scalar (string/int/float/bool)"
                ))
            })?;
            form = form.text(name, value);
        }
    }

    if let Some(files) = files {
        for (_, file) in files.iter() {
            let file = file
                .array()
                .ok_or_else(|| PhpException::default("each attachment must be an array".into()))?;

            // Distinguish a missing/null key from a present-but-non-string one
            // (a non-UTF8 string is present, not "missing").
            let name = match file.get("name").filter(|zv| !zv.is_null()) {
                None => return Err(PhpException::default("attachment is missing 'name'".into())),
                Some(zv) => zv
                    .str()
                    .ok_or_else(|| {
                        PhpException::default("attachment 'name' must be a UTF-8 string".into())
                    })?
                    .to_string(),
            };

            // Raw bytes — read via the zend_string so binary file content
            // survives (a PHP string is not necessarily valid UTF-8).
            // Distinguish a missing key from a present-but-wrong-type value.
            let contents_zv = file.get("contents").ok_or_else(|| {
                PhpException::default(format!("attachment '{name}' is missing 'contents'"))
            })?;
            let contents = contents_zv
                .zend_str()
                .ok_or_else(|| {
                    PhpException::default(format!(
                        "attachment '{name}' 'contents' must be a string"
                    ))
                })?
                .as_ref()
                .to_vec();

            let mut part = wreq::multipart::Part::bytes(contents);
            // Optional, but a present value of the wrong type must not be
            // silently dropped (a non-UTF8 latin1 filename would otherwise
            // vanish). A missing/null key leaves the field unset.
            if let Some(fzv) = file.get("filename").filter(|zv| !zv.is_null()) {
                let filename = fzv.str().ok_or_else(|| {
                    PhpException::default(format!(
                        "attachment '{name}' 'filename' must be a string"
                    ))
                })?;
                part = part.file_name(filename.to_string());
            }
            if let Some(czv) = file.get("content_type").filter(|zv| !zv.is_null()) {
                let mime = czv.str().ok_or_else(|| {
                    PhpException::default(format!(
                        "attachment '{name}' 'content_type' must be a string"
                    ))
                })?;
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
    if let Some(zv) = opt_present(opts, "emulation") {
        let (profile, nested_os) = if let Some(inner) = zv.array() {
            let profile = inner.get("profile").and_then(|z| z.str()).ok_or_else(|| {
                PhpException::default("emulation['profile'] must be a string".into())
            })?;
            // A present-but-wrong-type nested `os` must error, not silently
            // fall back to the top-level `emulation_os` (mirrors `profile`).
            let os = match inner.get("os").filter(|z| !z.is_null()) {
                Some(z) => Some(z.str().ok_or_else(|| {
                    PhpException::default("emulation['os'] must be a string".into())
                })?),
                None => None,
            };
            (profile, os)
        } else if let Some(name) = zv.str() {
            (name, None)
        } else {
            return Err(PhpException::default(
                "option 'emulation' must be a string or an array".into(),
            ));
        };

        let os = match nested_os {
            Some(v) => Some(v),
            None => opt_str(opts, "emulation_os")?,
        };
        let skip_http2 = opt_bool(opts, "skip_http2")?.unwrap_or(false);
        let skip_headers = opt_bool(opts, "skip_headers")?.unwrap_or(false);
        let config = if os.is_some() || skip_http2 || skip_headers {
            EmulationConfig::detailed(profile, os, skip_http2, skip_headers)
        } else {
            EmulationConfig::from_name(profile)
        }
        .map_err(PhpException::default)?;
        builder = config.apply(builder);
    }

    // ---- identity / default headers ----
    if let Some(ua) = opt_str(opts, "user_agent")? {
        builder = builder.user_agent(ua);
    }
    if let Some(zv) = opt_present(opts, "headers") {
        let table = zv
            .array()
            .ok_or_else(|| opt_type_error("headers", "an array (name => value)"))?;
        builder = builder.default_headers(headers_from_table(table).map_err(PhpException::from)?);
    }

    // ---- connection pool ----
    if let Some(n) = opt_long(opts, "pool_max_idle_per_host")? {
        builder = builder.pool_max_idle_per_host(
            checked_usize("pool_max_idle_per_host", n).map_err(PhpException::default)?,
        );
    }
    if let Some(n) = opt_long(opts, "pool_max_size")? {
        builder = builder
            .pool_max_size(checked_usize("pool_max_size", n).map_err(PhpException::default)?);
    }
    if let Some(secs) = opt_f64(opts, "pool_idle_timeout")? {
        // 0 = "no idle timeout" (skip the call), consistent with `timeout`.
        // Passing Duration::ZERO would instead evict every connection
        // immediately, effectively disabling the pool.
        let d = checked_duration("pool_idle_timeout", secs).map_err(PhpException::default)?;
        if !d.is_zero() {
            builder = builder.pool_idle_timeout(d);
        }
    }

    // ---- timeouts ----
    // Validate first (rejects NaN/±inf/negative via `checked_duration`), then
    // treat exactly 0 as "no timeout" and skip the builder call. The previous
    // `if secs > 0.0` guard swallowed -1/NaN silently, leaving the request with
    // no timeout at all.
    if let Some(secs) = opt_f64(opts, "timeout")? {
        let d = checked_duration("timeout", secs).map_err(PhpException::default)?;
        if !d.is_zero() {
            builder = builder.timeout(d);
        }
    }
    if let Some(secs) = opt_f64(opts, "read_timeout")? {
        let d = checked_duration("read_timeout", secs).map_err(PhpException::default)?;
        if !d.is_zero() {
            builder = builder.read_timeout(d);
        }
    }
    if let Some(secs) = opt_f64(opts, "connect_timeout")? {
        let d = checked_duration("connect_timeout", secs).map_err(PhpException::default)?;
        if !d.is_zero() {
            builder = builder.connect_timeout(d);
        }
    }

    // ---- cookies ----
    if opt_bool(opts, "cookies")?.unwrap_or(false) {
        builder = builder.cookie_store(true);
    }

    // ---- compression ----
    if let Some(v) = opt_bool(opts, "gzip")? {
        builder = builder.gzip(v);
    }
    if let Some(v) = opt_bool(opts, "brotli")? {
        builder = builder.brotli(v);
    }
    if let Some(v) = opt_bool(opts, "zstd")? {
        builder = builder.zstd(v);
    }
    if let Some(v) = opt_bool(opts, "deflate")? {
        builder = builder.deflate(v);
    }

    // ---- redirects ----
    if let Some(max) = opt_long(opts, "max_redirects")? {
        builder = builder.redirect(if max <= 0 {
            wreq::redirect::Policy::none()
        } else {
            wreq::redirect::Policy::limited(max as usize)
        });
    }
    if let Some(v) = opt_bool(opts, "referer")? {
        builder = builder.referer(v);
    }

    // ---- HTTP version ----
    if opt_bool(opts, "http1_only")?.unwrap_or(false) {
        builder = builder.http1_only();
    }
    if opt_bool(opts, "http2_only")?.unwrap_or(false) {
        builder = builder.http2_only();
    }
    if let Some(v) = opt_bool(opts, "https_only")? {
        builder = builder.https_only(v);
    }
    if let Some(v) = opt_bool(opts, "connection_verbose")? {
        builder = builder.connection_verbose(v);
    }

    // ---- TCP ----
    if let Some(v) = opt_bool(opts, "tcp_nodelay")? {
        builder = builder.tcp_nodelay(v);
    }
    if let Some(v) = opt_bool(opts, "tcp_reuse_address")? {
        builder = builder.tcp_reuse_address(v);
    }
    if let Some(secs) = opt_f64(opts, "tcp_keepalive")? {
        let d = checked_duration("tcp_keepalive", secs).map_err(PhpException::default)?;
        if !d.is_zero() {
            builder = builder.tcp_keepalive(d);
        }
    }
    if let Some(secs) = opt_f64(opts, "tcp_keepalive_interval")? {
        let d = checked_duration("tcp_keepalive_interval", secs).map_err(PhpException::default)?;
        if !d.is_zero() {
            builder = builder.tcp_keepalive_interval(d);
        }
    }
    // `tcp_user_timeout` is a Linux-family socket option (TCP_USER_TIMEOUT).
    #[cfg(any(target_os = "linux", target_os = "android", target_os = "fuchsia"))]
    if let Some(secs) = opt_f64(opts, "tcp_user_timeout")? {
        let d = checked_duration("tcp_user_timeout", secs).map_err(PhpException::default)?;
        if !d.is_zero() {
            builder = builder.tcp_user_timeout(d);
        }
    }
    if let Some(secs) = opt_f64(opts, "tcp_happy_eyeballs_timeout")? {
        let d =
            checked_duration("tcp_happy_eyeballs_timeout", secs).map_err(PhpException::default)?;
        if !d.is_zero() {
            builder = builder.tcp_happy_eyeballs_timeout(d);
        }
    }
    if let Some(n) = opt_long(opts, "tcp_keepalive_retries")? {
        builder = builder.tcp_keepalive_retries(
            checked_u32("tcp_keepalive_retries", n).map_err(PhpException::default)?,
        );
    }
    if let Some(n) = opt_long(opts, "tcp_send_buffer_size")? {
        builder = builder.tcp_send_buffer_size(
            checked_usize("tcp_send_buffer_size", n).map_err(PhpException::default)?,
        );
    }
    if let Some(n) = opt_long(opts, "tcp_recv_buffer_size")? {
        builder = builder.tcp_recv_buffer_size(
            checked_usize("tcp_recv_buffer_size", n).map_err(PhpException::default)?,
        );
    }

    // ---- network ----
    if let Some(addr) = opt_str(opts, "local_address")? {
        let ip: std::net::IpAddr = addr
            .parse()
            .map_err(|_| PhpException::default(format!("invalid local_address IP: '{addr}'")))?;
        builder = builder.local_address(ip);
    }
    #[cfg(unix)]
    if let Some(iface) = opt_str(opts, "interface")? {
        builder = builder.interface(iface.to_string());
    }
    if let Some(url) = opt_str(opts, "proxy")? {
        let proxy = wreq::Proxy::all(url)
            .map_err(|e| PhpException::default(format!("invalid proxy '{url}': {e}")))?;
        builder = builder.proxy(proxy);
    }
    if opt_bool(opts, "no_proxy")?.unwrap_or(false) {
        builder = builder.no_proxy();
    }
    if opt_bool(opts, "no_hickory_dns")?.unwrap_or(false) {
        builder = builder.no_hickory_dns();
    }
    if let Some(zv) = opt_present(opts, "resolve") {
        let table = zv
            .array()
            .ok_or_else(|| opt_type_error("resolve", "an array (host => \"ip:port\")"))?;
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
    if let Some(verify) = opt_bool(opts, "verify")? {
        builder = builder
            .tls_cert_verification(verify)
            .tls_verify_hostname(verify);
    }
    if let Some(v) = opt_bool(opts, "tls_sni")? {
        builder = builder.tls_sni(v);
    }
    if let Some(v) = opt_bool(opts, "tls_info")? {
        builder = builder.tls_info(v);
    }
    if let Some(v) = opt_str(opts, "min_tls_version")? {
        builder = builder.tls_min_version(parse_tls_version(v).map_err(PhpException::default)?);
    }
    if let Some(v) = opt_str(opts, "max_tls_version")? {
        builder = builder.tls_max_version(parse_tls_version(v).map_err(PhpException::default)?);
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
    "max_download_bytes",
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

    if opt_bool(opts, "http1_only")?.unwrap_or(false)
        && opt_bool(opts, "http2_only")?.unwrap_or(false)
    {
        return Err(PhpException::default(
            "options 'http1_only' and 'http2_only' are mutually exclusive".into(),
        ));
    }

    if opt_present(opts, "proxy").is_some() && opt_bool(opts, "no_proxy")?.unwrap_or(false) {
        return Err(PhpException::default(
            "options 'proxy' and 'no_proxy' are mutually exclusive".into(),
        ));
    }

    // Platform-specific options: known everywhere (so they are not flagged as
    // typos), but the builder call is `#[cfg]`-gated. On an unsupported OS they
    // would silently no-op — reject them explicitly instead.
    #[cfg(not(any(target_os = "linux", target_os = "android", target_os = "fuchsia")))]
    if opt_present(opts, "tcp_user_timeout").is_some() {
        return Err(PhpException::default(
            "option 'tcp_user_timeout' is only supported on Linux/Android/Fuchsia".into(),
        ));
    }
    #[cfg(not(unix))]
    if opt_present(opts, "interface").is_some() {
        return Err(PhpException::default(
            "option 'interface' is only supported on Unix".into(),
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

/// Coerces a PHP scalar (string/int/float/bool) to a string the way PHP's own
/// `(string)` cast would, for multipart text fields. Returns `None` for
/// non-scalars (arrays, objects) so the caller can reject them.
fn scalar_to_string(zv: &Zval) -> Option<String> {
    if let Some(s) = zv.str() {
        return Some(s.to_string());
    }
    if let Some(n) = zv.long() {
        return Some(n.to_string());
    }
    if let Some(f) = zv.double() {
        return Some(f.to_string());
    }
    if let Some(b) = zv.bool() {
        return Some(if b { "1".to_string() } else { String::new() });
    }
    None
}

/// Builds a "wrong type" error for an option that is present but not the
/// expected PHP type. Options with the wrong type used to be silently ignored;
/// we reject them so a typo like `timeout => "5"` or `no_hickory_dns => "true"`
/// surfaces instead of leaving the setting unapplied.
fn opt_type_error(key: &str, expected: &str) -> PhpException {
    PhpException::default(format!("client option '{key}' must be {expected}"))
}

/// A present, non-null option value. Returns `None` for a missing key or an
/// explicit `null` (both mean "not set"); `Some(zv)` otherwise.
fn opt_present<'a>(opts: &'a ZendHashTable, key: &str) -> Option<&'a Zval> {
    match opts.get(key) {
        Some(zv) if !zv.is_null() => Some(zv),
        _ => None,
    }
}

/// Reads an option as a bool. Errors if present but not a bool.
fn opt_bool(opts: &ZendHashTable, key: &str) -> PhpResult<Option<bool>> {
    match opt_present(opts, key) {
        None => Ok(None),
        Some(zv) => zv
            .bool()
            .map(Some)
            .ok_or_else(|| opt_type_error(key, "a bool")),
    }
}

/// Reads an option as an integer. Also accepts a float with no fractional
/// part (e.g. `3.0`), mirroring `opt_f64`'s acceptance of ints, so the two
/// helpers stay consistent. Errors if present but not integer-valued.
fn opt_long(opts: &ZendHashTable, key: &str) -> PhpResult<Option<i64>> {
    match opt_present(opts, key) {
        None => Ok(None),
        Some(zv) => {
            if let Some(n) = zv.long() {
                return Ok(Some(n));
            }
            // A whole-valued float within i64 range is accepted as an integer.
            // `i64::MAX as f64` rounds up to 2^63, which would saturate back to
            // i64::MAX (a silent -1) — use a strict `<` so 2^63 is rejected.
            // i64::MIN is exactly representable, so `>=` is correct there.
            if let Some(f) = zv.double() {
                if f.is_finite() && f.fract() == 0.0 && f >= i64::MIN as f64 && f < i64::MAX as f64
                {
                    return Ok(Some(f as i64));
                }
            }
            Err(opt_type_error(key, "an integer"))
        }
    }
}

/// Reads an option as a string slice. Errors if present but not a string.
fn opt_str<'a>(opts: &'a ZendHashTable, key: &str) -> PhpResult<Option<&'a str>> {
    match opt_present(opts, key) {
        None => Ok(None),
        Some(zv) => zv
            .str()
            .map(Some)
            .ok_or_else(|| opt_type_error(key, "a string")),
    }
}

/// Reads an option as a float (accepts PHP ints too). Errors if present but not
/// a number.
fn opt_f64(opts: &ZendHashTable, key: &str) -> PhpResult<Option<f64>> {
    match opt_present(opts, key) {
        None => Ok(None),
        Some(zv) => double_value(zv)
            .map(Some)
            .ok_or_else(|| opt_type_error(key, "a number")),
    }
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
