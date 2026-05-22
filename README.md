# wreq-php

A PHP HTTP client with **deterministic connection reuse** and **browser
TLS/HTTP2 fingerprinting**, built on the Rust [`wreq`](https://crates.io/crates/wreq)
crate.

## Why

`curl` (and therefore most PHP HTTP clients) reuses TCP connections through an
opaque, process-wide handle cache — you cannot reliably tell whether a keep-alive
connection will be reused, or whether it leaks across unrelated handles.

`wreq-php` makes connection reuse **explicit and deterministic**:

- **One `Client` owns one connection pool.** Reuse the same `$client` and its
  keep-alive TCP/TLS connections are reused. Use a separate `$client` and you
  get a separate, fully isolated pool.
- **`close()` (or letting the client go out of scope) tears the pool down** —
  every idle socket is closed immediately.
- **Browser emulation** — TLS/JA3/HTTP2 fingerprints of real browsers, sourced
  straight from `wreq-util` (100+ profiles, kept current with the crate).

## Architecture

Two layers in one repository:

| Layer | What it is | What it does |
|-------|-----------|--------------|
| Native extension (`wreq_php`) | Rust + `ext-php-rs` | Owns the `wreq::Client` and its pool; executes requests; raw response. |
| Composer package (`eav93/wreq-php`) | Pure PHP, `Wreq\*` | Laravel-style ergonomics: `Client`, immutable `PendingRequest`, `Response`. |

The native classes (`Wreq\Ext\*`) are a thin, fast core. The PHP layer wraps
them — that is where `json()`, `object()`, `resource()`, the status helpers and
the immutable per-request builder live.

## Installation

```bash
composer require eav93/wreq-php
```

Composer downloads a prebuilt native binary matching your PHP version, OS and
architecture. Enable it in `php.ini`:

```ini
extension=/path/printed/by/the/installer/wreq_php.so
```

No prebuilt binary for your platform? Build it from source:

```bash
cargo build --release
# then point php.ini at target/release/libwreq_php.so
```

### Docker

Ready-to-use images with the extension already compiled in and enabled are
published to the GitHub Container Registry. They mirror the official `php`
image line-up — same variants, same tag format — so they are drop-in base
images:

```dockerfile
FROM ghcr.io/eav93/wreq-php:8.3-cli
# the wreq_php extension is already loaded
COPY . /app
```

Every image is multi-arch (amd64 + arm64). Tags — every PHP version
`8.1`…`8.5` with each variant:

| Variant | Base | libc |
|---------|------|------|
| `<php>-cli`, `<php>-fpm`, `<php>-apache` | Debian | glibc |
| `<php>-cli-alpine`, `<php>-fpm-alpine` | Alpine | musl |

(ZTS PHP builds are not covered — the prebuilt extension is NTS-only.)

The published images carry the prebuilt extension from the matching release
(no recompilation). To extract just the musl binary yourself:

```bash
docker build -f docker/Dockerfile.alpine --build-arg PHP_VERSION=8.3 \
    --target artifact --output type=local,dest=dist .
```

### Adding the extension to your own image

If you would rather start from an official `php` image, drop the prebuilt
binary in — no compilation, just a download. The snippet auto-detects the PHP
version, architecture and libc, so it works on any `php:<ver>-cli` /
`-fpm` / `-apache` (Debian) or `-alpine` tag:

```dockerfile
FROM php:8.3-cli

ARG WREQ_PHP_VERSION=v0.1.7
RUN set -eux; \
    if [ -f /etc/alpine-release ]; then \
        apk add --no-cache curl libstdc++ libgcc; libc=musl; \
    else \
        apt-get update && apt-get install -y --no-install-recommends curl ca-certificates; \
        rm -rf /var/lib/apt/lists/*; libc=gnu; \
    fi; \
    php_ver="$(php -r 'echo PHP_MAJOR_VERSION.".".PHP_MINOR_VERSION;')"; \
    arch="$(uname -m)"; [ "$arch" = arm64 ] && arch=aarch64; \
    curl -fsSL -o "$(php-config --extension-dir)/wreq_php.so" \
        "https://github.com/eav93/wreq-php/releases/download/${WREQ_PHP_VERSION}/wreq_php-php${php_ver}-nts-linux-${libc}-${arch}.so"; \
    docker-php-ext-enable wreq_php; \
    php -m | grep -q wreq_php
```

The extension links `libstdc++`/`libgcc` (Alpine needs them installed
explicitly, as above). Use `releases/latest/download/` instead of a pinned
tag to always pull the newest build. ZTS PHP builds are not covered — the
prebuilt binary is NTS.

## Usage

```php
use Wreq\Client;

// One reusable client === one connection pool.
$client = new Client([
    'emulation'              => 'chrome_131', // browser fingerprint
    'pool_max_idle_per_host' => 8,            // TCP connections kept per host
    'cookies'                => true,         // shared cookie jar
    'timeout'                => 30.0,
]);

$response = $client->get('https://api.example.com/users', ['page' => 1]);

$response->status();          // int
$response->successful();      // 2xx?
$response->body();            // string
$response->json('data.0.name', 'default'); // dot-notation + default
$response->object();          // stdClass graph
$response->header('Content-Type');

// POST JSON (default) — connections reused from the same pool.
$client->post('https://api.example.com/users', ['name' => 'Ada']);

// Per-request tweaks return a new immutable builder; the pool is untouched.
$client->asForm()->post($url, ['field' => 'value']);
$client->withToken('secret')->withHeaders(['X-Trace' => '1'])->get($url);

// multipart/form-data — attach files alongside text fields.
$client->attach('photo', file_get_contents('p.jpg'), 'p.jpg', 'image/jpeg')
       ->post($url, ['caption' => 'Sunset']);

// Release the pool and close every idle socket now.
$client->close();
```

### Client options

Every `wreq::ClientBuilder` setting expressible as a PHP scalar is supported.

| Option | Type | Meaning |
|--------|------|---------|
| `emulation` | string | Browser profile (`chrome_131`, `firefox_136`, …). |
| `emulation_os` | string | `windows`, `macos`, `linux`, `android`, `ios`. |
| `skip_http2` / `skip_headers` | bool | Emulation fingerprint toggles. |
| `user_agent` | string | Default `User-Agent`. |
| `headers` | array | Default headers for every request. |
| `base_url` | string | Prefix for relative request paths. |
| `pool_max_idle_per_host` | int | Idle keep-alive sockets kept per host. |
| `pool_max_size` | int | Max total connections in the pool. |
| `pool_idle_timeout` | float | Idle socket lifetime, seconds. |
| `timeout` / `read_timeout` / `connect_timeout` | float | Request / body-read / connect timeouts, seconds. |
| `cookies` | bool | Enable a per-client cookie jar. |
| `gzip` / `brotli` / `zstd` / `deflate` | bool | Toggle response decompression. |
| `max_redirects` | int | Redirect limit; `0` disables following. |
| `referer` | bool | Auto-set the `Referer` header. |
| `http1_only` / `http2_only` / `https_only` | bool | Restrict protocol / scheme. |
| `connection_verbose` | bool | Verbose connection tracing. |
| `tcp_nodelay` / `tcp_reuse_address` | bool | TCP socket options. |
| `tcp_keepalive` / `tcp_keepalive_interval` / `tcp_user_timeout` / `tcp_happy_eyeballs_timeout` | float | TCP timers, seconds. |
| `tcp_keepalive_retries` | int | TCP keep-alive probe count. |
| `tcp_send_buffer_size` / `tcp_recv_buffer_size` | int | Socket buffer sizes, bytes. |
| `local_address` | string | Bind to a local IP. |
| `interface` | string | Bind to a network interface (Unix only). |
| `proxy` | string | Proxy URL for all requests. |
| `no_proxy` | bool | Ignore proxies, including system ones. |
| `no_hickory_dns` | bool | Use the system DNS resolver. |
| `resolve` | array | DNS overrides, `host => "ip:port"`. |
| `verify` | bool | Verify TLS certificate and hostname (default `true`). |
| `tls_sni` / `tls_info` | bool | TLS SNI / expose TLS info. |
| `min_tls_version` / `max_tls_version` | string | `"1.0"`–`"1.3"`. |

Settings that need Rust objects (client certificates, custom cert/DNS stores,
HTTP/2 frame tuning, tower layers) are not exposed — use the `wreq` crate
directly for those.

### Emulation profiles

```php
use Wreq\Emulation;

Emulation::all();              // every supported profile name
Emulation::random();           // a random profile
Emulation::exists('chrome_131');
```

## Development

Building the extension needs a PHP install with development files
(`php-config` and headers). On macOS, `brew install php` provides them;
Laravel Herd's PHP does not. Point the build at it if it is not on `PATH`:

```bash
export PHP_CONFIG=$(command -v php-config)
cargo build --release                        # build the extension
composer install                             # PHP dev dependencies
php -d extension=./target/release/libwreq_php.so vendor/bin/phpunit
```

The pure-PHP test suite runs without the extension; integration tests are
skipped automatically when it is not loaded.

### Vendored ext-php-rs-bindgen

`ext-php-rs` 0.15.13 generates its PHP bindings with `ext-php-rs-bindgen`,
which depends on a forked `ext-php-rs-clang-sys`. That fork keeps
`links = "clang"`, which collides with the regular `clang-sys` reached through
`wreq`'s `boring-sys2` — Cargo forbids two packages with the same `links`, so
the two cannot otherwise coexist.

`third_party/ext-php-rs-bindgen` is a vendored copy whose only change re-points
that dependency at the upstream `clang-sys` (the fork is unnecessary —
`preserve_none` is handled numerically, not via a fork-only constant). It is
wired in via `[patch.crates-io]`, leaving a single `links = "clang"` in the
graph. See [extphprs/ext-php-rs#740](https://github.com/extphprs/ext-php-rs/issues/740).

## License

**LGPL-3.0-or-later.** The browser-emulation crate `wreq-util` is LGPL-3.0, and
the prebuilt binaries statically link it, so the project as a whole is
LGPL-3.0. See [`LICENSE`](LICENSE) and
[`THIRD_PARTY_LICENSES.md`](THIRD_PARTY_LICENSES.md) for details and the
relinking provisions.
