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

Install the binary with the Composer installer **inside your runtime stage**.
It downloads the `.so` from the same GitHub release as the Composer package, so
the native binary and the PHP wrapper can never drift apart.

```dockerfile
FROM php:8.3-cli

WORKDIR /app
COPY . /app
# ... install your Composer dependencies (vendor/) as you normally would ...

# Fetch the matching wreq_php binary and enable it. The installer picks the
# build for this image's PHP version, OS, libc and architecture.
RUN php -r 'require "vendor/autoload.php"; Wreq\Installer::run();' \
    && cp vendor/eav93/wreq-php/runtime/wreq_php-*.so "$(php-config --extension-dir)/wreq_php.so" \
    && docker-php-ext-enable wreq_php
```

Run the installer in the **final image**, not in a separate `composer` build
stage — it must see the PHP version, libc and architecture the extension will
actually run under (a `composer` image has a different PHP). It reads the
installed package version through Composer and downloads
`wreq_php-php<X>-nts-<os>-<arch>.so` from that exact release, verifying the
checksum. Bump the version in one place — `composer.lock` — and the next build
fetches the matching binary. No `curl` needed: the installer uses PHP's HTTP
wrapper (`allow_url_fopen`, on by default in the official `php` images).

As a safety net the library also compares, on first use, the binary version
(`Wreq\Client::extensionVersion()`) against the package version and throws
`Wreq\Exceptions\VersionMismatchException` on a major.minor mismatch.

ZTS PHP builds are not covered — the prebuilt binary is NTS.

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
| `proxy` | string | Proxy URL for all requests (`http://`, `https://`, `socks4://`, `socks4a://`, `socks5://`, `socks5h://`). |
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
Emulation::random('chrome');   // a random Chrome version
Emulation::like('firefox');    // all Firefox profile names
Emulation::exists('chrome_131');
```

A random browser version with a fixed OS:

```php
new Wreq\Client(['emulation' => [
    'profile' => Emulation::random('chrome'),
    'os'      => 'windows',
]]);
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

## License

**LGPL-3.0-or-later.** The browser-emulation crate `wreq-util` is LGPL-3.0, and
the prebuilt binaries statically link it, so the project as a whole is
LGPL-3.0. See [`LICENSE`](LICENSE) and
[`THIRD_PARTY_LICENSES.md`](THIRD_PARTY_LICENSES.md) for details and the
relinking provisions.
