<?php

/**
 * IDE / static-analysis stubs for the native `wreq_php` extension.
 *
 * These classes are implemented in Rust and registered at runtime. This file
 * is never executed — it exists only so editors and tools resolve the
 * `Wreq\Ext\*` symbols used by the PHP layer.
 */

declare(strict_types=1);

namespace Wreq\Ext;

/**
 * Native reusable HTTP client. Owns one `wreq::Client` and its connection pool.
 */
final class Client
{
    /**
     * @param  array<string, mixed>|null  $options
     */
    public function __construct(?array $options = null) {}

    /**
     * Executes an HTTP request and returns the response with its body read.
     *
     * @param  array<string, string>|null  $headers
     */
    public function request(string $method, string $url, ?array $headers = null, ?string $body = null): Response {}

    /**
     * Executes a `multipart/form-data` request.
     *
     * @param  array<string, string>|null  $headers
     * @param  array<string, mixed>|null  $fields
     * @param  array<int, array{name: string, contents: string, filename?: string, content_type?: string}>|null  $files
     */
    public function requestMultipart(string $method, string $url, ?array $headers = null, ?array $fields = null, ?array $files = null): Response {}

    /**
     * Releases the connection pool, closing all idle keep-alive sockets.
     */
    public function close(): void {}

    /**
     * Whether the client is still usable (not yet closed).
     */
    public function isOpen(): bool {}

    /**
     * The release version the native extension was built from.
     */
    public static function extensionVersion(): string {}
}

/**
 * Native HTTP response. Raw view: status, headers and body bytes.
 */
final class Response
{
    public function status(): int {}

    public function version(): string {}

    public function url(): string {}

    public function body(): string {}

    /**
     * @return array<string, array<int, string>>
     */
    public function headers(): array {}

    public function header(string $name): ?string {}

    public function remoteAddr(): ?string {}
}

/**
 * Native registry of browser emulation profiles.
 */
final class Emulation
{
    /**
     * @return array<int, string>
     */
    public static function all(): array {}

    public static function random(): string {}

    public static function exists(string $name): bool {}
}

/** Base class for every error raised by the extension. */
class RequestException extends \Exception {}

/** Connection could not be established (DNS, refused, reset). */
class ConnectionException extends RequestException {}

/** Request exceeded its timeout. */
class TimeoutException extends RequestException {}

/** TLS handshake / certificate failure. */
class TlsException extends RequestException {}

/** Redirect policy was violated (loop or limit exceeded). */
class RedirectException extends RequestException {}

/** Raised by `Response::throw()` for a 4xx/5xx status. */
class StatusException extends RequestException {}
