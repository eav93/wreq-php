<?php

declare(strict_types=1);

namespace Wreq;

/**
 * Immutable per-request builder.
 *
 * Every `with*`/`as*`/`accept*` method returns a **new** instance — the parent
 * `Client` and, crucially, its connection pool are never mutated. All requests
 * built from the same `Client` share that one pool, so keep-alive TCP/TLS
 * connections are reused deterministically.
 */
final class PendingRequest
{
    /** @var array<string, string> Accumulated request headers. */
    private array $headers = [];

    /** @var array<string, mixed> Accumulated query parameters. */
    private array $query = [];

    /** Body encoding for array payloads: `json` or `form`. */
    private string $bodyFormat = 'json';

    /** Explicit raw body set via `withBody()`, bypassing array encoding. */
    private ?string $rawBody = null;

    /**
     * @param  \Wreq\Ext\Client  $ext  Shared native client (and its pool).
     * @param  string  $baseUrl  Optional base URL prepended to relative paths.
     */
    public function __construct(
        private readonly object $ext,
        private readonly string $baseUrl = '',
    ) {
    }

    /**
     * Adds request headers.
     *
     * @param  array<string, string>  $headers
     */
    public function withHeaders(array $headers): self
    {
        $clone = clone $this;
        $clone->headers = array_merge($this->headers, $headers);

        return $clone;
    }

    /**
     * Adds a single request header.
     */
    public function withHeader(string $name, string $value): self
    {
        return $this->withHeaders([$name => $value]);
    }

    /**
     * Adds an `Authorization` header (Bearer by default).
     */
    public function withToken(string $token, string $type = 'Bearer'): self
    {
        return $this->withHeader('Authorization', trim($type.' '.$token));
    }

    /**
     * Adds an HTTP Basic `Authorization` header.
     */
    public function withBasicAuth(string $username, string $password): self
    {
        return $this->withHeader('Authorization', 'Basic '.base64_encode($username.':'.$password));
    }

    /**
     * Overrides the `User-Agent` header.
     */
    public function withUserAgent(string $userAgent): self
    {
        return $this->withHeader('User-Agent', $userAgent);
    }

    /**
     * Sets the `Accept` header.
     */
    public function accept(string $contentType): self
    {
        return $this->withHeader('Accept', $contentType);
    }

    /**
     * Sets `Accept: application/json`.
     */
    public function acceptJson(): self
    {
        return $this->accept('application/json');
    }

    /**
     * Merges query parameters applied to every request.
     *
     * @param  array<string, mixed>  $query
     */
    public function withQuery(array $query): self
    {
        $clone = clone $this;
        $clone->query = array_merge($this->query, $query);

        return $clone;
    }

    /**
     * Encodes array payloads as `application/x-www-form-urlencoded`.
     */
    public function asForm(): self
    {
        $clone = clone $this;
        $clone->bodyFormat = 'form';

        return $clone;
    }

    /**
     * Encodes array payloads as JSON (the default).
     */
    public function asJson(): self
    {
        $clone = clone $this;
        $clone->bodyFormat = 'json';

        return $clone;
    }

    /**
     * Sends a raw, pre-encoded body.
     */
    public function withBody(string $content, string $contentType = 'application/json'): self
    {
        $clone = clone $this;
        $clone->rawBody = $content;

        return $clone->withHeader('Content-Type', $contentType);
    }

    /**
     * Sends a GET request.
     *
     * @param  array<string, mixed>  $query
     */
    public function get(string $url, array $query = []): Response
    {
        return $this->withQuery($query)->dispatch('GET', $url, null);
    }

    /**
     * Sends a HEAD request.
     *
     * @param  array<string, mixed>  $query
     */
    public function head(string $url, array $query = []): Response
    {
        return $this->withQuery($query)->dispatch('HEAD', $url, null);
    }

    /**
     * Sends a POST request.
     *
     * @param  array<string, mixed>|string  $data
     */
    public function post(string $url, array|string $data = []): Response
    {
        return $this->dispatch('POST', $url, $data);
    }

    /**
     * Sends a PUT request.
     *
     * @param  array<string, mixed>|string  $data
     */
    public function put(string $url, array|string $data = []): Response
    {
        return $this->dispatch('PUT', $url, $data);
    }

    /**
     * Sends a PATCH request.
     *
     * @param  array<string, mixed>|string  $data
     */
    public function patch(string $url, array|string $data = []): Response
    {
        return $this->dispatch('PATCH', $url, $data);
    }

    /**
     * Sends a DELETE request.
     *
     * @param  array<string, mixed>|string  $data
     */
    public function delete(string $url, array|string $data = []): Response
    {
        return $this->dispatch('DELETE', $url, $data);
    }

    /**
     * Sends a request with an arbitrary HTTP method.
     *
     * @param  array<string, mixed>|string|null  $data
     */
    public function send(string $method, string $url, array|string|null $data = null): Response
    {
        return $this->dispatch(strtoupper($method), $url, $data);
    }

    /**
     * Builds and executes the request, wrapping the native response.
     *
     * @param  array<string, mixed>|string|null  $data
     */
    private function dispatch(string $method, string $url, array|string|null $data): Response
    {
        $headers = $this->headers;
        $body = null;

        if ($this->rawBody !== null) {
            $body = $this->rawBody;
        } elseif (is_string($data)) {
            $body = $data;
        } elseif (is_array($data) && $data !== []) {
            if ($this->bodyFormat === 'form') {
                $body = http_build_query($data);
                $headers += ['Content-Type' => 'application/x-www-form-urlencoded'];
            } else {
                $body = json_encode($data, JSON_THROW_ON_ERROR | JSON_UNESCAPED_SLASHES);
                $headers += ['Content-Type' => 'application/json'];
            }
        }

        $raw = $this->ext->request($method, $this->buildUrl($url), $headers, $body);

        return new Response($raw);
    }

    /**
     * Resolves the final URL: applies the base URL and appends query params.
     */
    private function buildUrl(string $url): string
    {
        if ($this->baseUrl !== '' && ! preg_match('#^https?://#i', $url)) {
            $url = rtrim($this->baseUrl, '/').'/'.ltrim($url, '/');
        }

        if ($this->query === []) {
            return $url;
        }

        $separator = str_contains($url, '?') ? '&' : '?';

        return $url.$separator.http_build_query($this->query);
    }
}
