<?php

// Stubs for wreq_php

namespace Wreq\Ext {
    /**
     * Reusable HTTP client with a dedicated connection pool.
     */
    class Client {
        /**
         * Builds a client from an options array. See `build_wreq_client` for the
         * supported keys.
         *
         * @param array|null $options
         */
        public function __construct(?array $options = null) {}

        /**
         * Releases the connection pool now, closing all idle keep-alive sockets.
         * Subsequent requests through this client raise an exception.
         *
         * @return void
         */
        public function close(): void {}

        /**
         * The release version the native extension was built from (e.g. `0.1.9`),
         * or `0.0.0-dev` for a local build. The PHP layer compares it with the
         * Composer package version to detect a binary/wrapper mismatch.
         *
         * @return string
         */
        public static function extensionVersion(): string {}

        /**
         * Whether the client is still usable (not yet closed).
         *
         * @return bool
         */
        public function isOpen(): bool {}

        /**
         * Executes an HTTP request and returns the response with its body read.
         *
         * * `method`  — HTTP method (`GET`, `POST`, …).
         * * `url`     — fully-formed URL (the PHP layer appends any query string).
         * * `headers` — per-request headers (`name => value`).
         * * `body`    — raw request body, already encoded by the PHP layer.
         *
         * @param string $method
         * @param string $url
         * @param array|null $headers
         * @param string|null $body
         * @return \Wreq\Ext\Response
         */
        public function request(string $method, string $url, ?array $headers = null, ?string $body = null): \Wreq\Ext\Response {}

        /**
         * Executes a `multipart/form-data` request.
         *
         * * `fields` — text fields (`name => value`).
         * * `files`  — a list of attachments; each is an array with `name`,
         *   `contents` (raw bytes) and optional `filename` / `content_type`.
         *
         * @param string $method
         * @param string $url
         * @param array|null $headers
         * @param array|null $fields
         * @param array|null $files
         * @return \Wreq\Ext\Response
         */
        public function requestMultipart(string $method, string $url, ?array $headers = null, ?array $fields = null, ?array $files = null): \Wreq\Ext\Response {}
    }

    /**
     * Connection could not be established (DNS, refused, reset).
     */
    class ConnectionException extends \Wreq\Ext\RequestException {
        public function __construct() {}
    }

    /**
     * Static registry of available emulation profiles, exposed to PHP.
     */
    class Emulation {
        public function __construct() {}

        /**
         * Every supported profile name (e.g. `chrome_131`, `firefox_136`).
         *
         * @return array
         */
        public static function all(): array {}

        /**
         * Whether the given profile name is recognized.
         *
         * @param string $name
         * @return bool
         */
        public static function exists(string $name): bool {}

        /**
         * A random profile name.
         *
         * @return string
         */
        public static function random(): string {}
    }

    /**
     * Redirect policy was violated (loop or limit exceeded).
     */
    class RedirectException extends \Wreq\Ext\RequestException {
        public function __construct() {}
    }

    /**
     * Base class for every error raised by the extension. Extends `\Exception`.
     */
    class RequestException extends \Exception {
        public function __construct() {}
    }

    /**
     * HTTP response returned by `Client::request()`.
     */
    class Response {
        public function __construct() {}

        /**
         * Raw response body as a binary-safe PHP string.
         *
         * @return mixed
         */
        public function body(): mixed {}

        /**
         * A single header by name (case-insensitive); multiple values are joined
         * with `", "`. Returns `null` when the header is absent.
         *
         * @param string $name
         * @return string|null
         */
        public function header(string $name): ?string {}

        /**
         * All headers as a map of lowercased name => list of values.
         *
         * @return array
         */
        public function headers(): array {}

        /**
         * Remote peer address (`ip:port`) the response came from, if known.
         *
         * @return string|null
         */
        public function remoteAddr(): ?string {}

        /**
         * HTTP status code (e.g. 200, 404).
         *
         * @return int
         */
        public function status(): int {}

        /**
         * Final URL after any redirects.
         *
         * @return string
         */
        public function url(): string {}

        /**
         * HTTP protocol version string (e.g. `"HTTP/2.0"`).
         *
         * @return string
         */
        public function version(): string {}
    }

    /**
     * Raised by `Response::throw()` for a 4xx/5xx status.
     */
    class StatusException extends \Wreq\Ext\RequestException {
        public function __construct() {}
    }

    /**
     * Request exceeded its timeout.
     */
    class TimeoutException extends \Wreq\Ext\RequestException {
        public function __construct() {}
    }

    /**
     * TLS handshake / certificate failure.
     */
    class TlsException extends \Wreq\Ext\RequestException {
        public function __construct() {}
    }
}
