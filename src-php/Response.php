<?php

declare(strict_types=1);

namespace Wreq;

use Wreq\Exceptions\RequestException;

/**
 * HTTP response with Laravel-style accessors.
 *
 * Wraps the raw native response (`Wreq\Ext\Response`) and adds JSON decoding,
 * status helpers and stream access. The body is already fully read, so every
 * accessor is safe to call repeatedly.
 */
final class Response
{
    /** Whether the JSON body has been decoded and cached yet. */
    private bool $jsonDecoded = false;

    /** Cached decoded JSON body (only meaningful once $jsonDecoded is true). */
    private mixed $jsonValue = null;

    /**
     * @param  Ext\Response  $raw  The native response object.
     * @param  string|null  $sink  Path the body was streamed to, if any.
     */
    public function __construct(
        private readonly object $raw,
        private readonly ?string $sink = null,
    ) {}

    /**
     * Raw response body as a string.
     *
     * Empty for a streamed (`sink`) response — the body was written to disk
     * instead of being held in memory; use {@see savedTo()} for its path.
     */
    public function body(): string
    {
        return $this->raw->body();
    }

    /**
     * Path the response body was streamed to via `sink()`, or `null` for an
     * ordinary in-memory response.
     */
    public function savedTo(): ?string
    {
        return $this->sink;
    }

    /**
     * Number of bytes written to disk for a streamed (`sink`) response, or
     * `null` for an ordinary in-memory response.
     */
    public function downloadedBytes(): ?int
    {
        return $this->raw->downloadedBytes();
    }

    /**
     * Decoded JSON body.
     *
     * With no argument the whole payload is returned (as an associative array).
     * With a `$key` in dot notation a nested value is returned, falling back to
     * `$default` when the path is missing.
     *
     * A malformed body raises `\JsonException` — distinct from a literal JSON
     * `null` (returned as `null`) and from a missing key (returns `$default`).
     *
     *
     * @throws \JsonException
     */
    public function json(?string $key = null, mixed $default = null): mixed
    {
        if (! $this->jsonDecoded) {
            $this->jsonValue = json_decode($this->body(), true, 512, JSON_THROW_ON_ERROR);
            $this->jsonDecoded = true;
        }

        if ($key === null) {
            return $this->jsonValue;
        }

        return self::dataGet($this->jsonValue, $key, $default);
    }

    /**
     * Decoded JSON body as a `stdClass` object graph.
     *
     * @throws \JsonException on a malformed body.
     */
    public function object(): mixed
    {
        return json_decode($this->body(), false, 512, JSON_THROW_ON_ERROR);
    }

    /**
     * Body exposed as an in-memory stream resource.
     *
     * @return resource
     */
    public function resource()
    {
        $stream = fopen('php://temp', 'r+b');

        if ($stream === false) {
            throw new \RuntimeException('Unable to open an in-memory stream for the response body.');
        }

        fwrite($stream, $this->body());
        rewind($stream);

        return $stream;
    }

    /**
     * HTTP status code.
     */
    public function status(): int
    {
        return $this->raw->status();
    }

    /**
     * HTTP protocol version (e.g. "HTTP/2.0").
     */
    public function version(): string
    {
        return $this->raw->version();
    }

    /**
     * Final URL after any redirects.
     */
    public function url(): string
    {
        return $this->raw->url();
    }

    /**
     * Remote peer address (`ip:port`) the response came from, if known.
     */
    public function remoteAddr(): ?string
    {
        return $this->raw->remoteAddr();
    }

    /**
     * A single header value (case-insensitive); empty string when absent.
     */
    public function header(string $header): string
    {
        return $this->raw->header($header) ?? '';
    }

    /**
     * All headers as `name => [values]` (names lowercased).
     *
     * @return array<string, array<int, string>>
     */
    public function headers(): array
    {
        return $this->raw->headers();
    }

    /**
     * Status is exactly 200.
     */
    public function ok(): bool
    {
        return $this->status() === 200;
    }

    /**
     * Status is 2xx.
     */
    public function successful(): bool
    {
        return $this->status() >= 200 && $this->status() < 300;
    }

    /**
     * Status is 3xx.
     */
    public function redirect(): bool
    {
        return $this->status() >= 300 && $this->status() < 400;
    }

    /**
     * Status is 4xx or 5xx.
     */
    public function failed(): bool
    {
        return $this->clientError() || $this->serverError();
    }

    /**
     * Status is 4xx.
     */
    public function clientError(): bool
    {
        return $this->status() >= 400 && $this->status() < 500;
    }

    /**
     * Status is 5xx.
     */
    public function serverError(): bool
    {
        return $this->status() >= 500 && $this->status() < 600;
    }

    /**
     * Throws a {@see RequestException} when the response failed (4xx/5xx).
     *
     * @return $this
     */
    public function throw(): self
    {
        if ($this->failed()) {
            throw new RequestException($this);
        }

        return $this;
    }

    /**
     * The underlying native response object.
     */
    public function raw(): object
    {
        return $this->raw;
    }

    /**
     * Resolves a dot-notation key against a decoded structure.
     */
    private static function dataGet(mixed $target, string $key, mixed $default): mixed
    {
        foreach (explode('.', $key) as $segment) {
            if (is_array($target) && array_key_exists($segment, $target)) {
                $target = $target[$segment];
            } elseif ($target instanceof \ArrayAccess && isset($target[$segment])) {
                $target = $target[$segment];
            } elseif (is_object($target) && isset($target->{$segment})) {
                $target = $target->{$segment};
            } else {
                return $default;
            }
        }

        return $target;
    }
}
