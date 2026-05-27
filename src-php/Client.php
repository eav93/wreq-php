<?php

declare(strict_types=1);

namespace Wreq;

use Wreq\Ext\RequestException;
use Wreq\Support\Extension;

/**
 * Reusable HTTP client.
 *
 * One `Client` owns exactly one native connection pool. Keep a `Client` around
 * and its keep-alive TCP/TLS connections are reused across requests; create a
 * separate `Client` and you get a separate, fully isolated pool. Unlike curl's
 * shared handle cache, reuse here is explicit and deterministic.
 *
 * Configuration is fixed at construction. Per-request tweaks go through the
 * immutable {@see PendingRequest} returned by the `with*`/`as*` methods, so
 * they never disturb the shared pool.
 *
 * ```php
 * $client = new Wreq\Client([
 *     'emulation'              => 'chrome_131',
 *     'pool_max_idle_per_host' => 8,
 *     'cookies'                => true,
 *     'timeout'                => 30.0,
 * ]);
 *
 * $response = $client->get('https://api.example.com/users', ['page' => 1]);
 * $users    = $response->json('data');
 * ```
 */
final class Client
{
    /** Native client holding the `wreq::Client` and its connection pool. */
    private readonly object $ext;

    /** Base URL prepended to relative request paths. */
    private readonly string $baseUrl;

    /**
     * @param  array<string, mixed>  $options  Client options. Every
     *                                         `wreq::ClientBuilder` setting expressible as a scalar is supported —
     *                                         emulation, connection pool, timeouts, cookies, compression, redirects,
     *                                         HTTP version, TCP tuning, proxy, DNS and TLS — plus `base_url`. See the
     *                                         README "Client options" table for the full list.
     */
    public function __construct(array $options = [])
    {
        Extension::ensure();

        $this->baseUrl = (string) ($options['base_url'] ?? '');
        $this->ext = new Ext\Client($options);
    }

    /**
     * Starts an immutable request builder with the given headers.
     *
     * @param  array<string, string>  $headers
     */
    public function withHeaders(array $headers): PendingRequest
    {
        return $this->newRequest()->withHeaders($headers);
    }

    /**
     * Starts an immutable request builder with a single header.
     */
    public function withHeader(string $name, string $value): PendingRequest
    {
        return $this->newRequest()->withHeader($name, $value);
    }

    /**
     * Starts an immutable request builder with a Bearer token.
     */
    public function withToken(string $token, string $type = 'Bearer'): PendingRequest
    {
        return $this->newRequest()->withToken($token, $type);
    }

    /**
     * Starts an immutable request builder with HTTP Basic auth.
     */
    public function withBasicAuth(string $username, string $password): PendingRequest
    {
        return $this->newRequest()->withBasicAuth($username, $password);
    }

    /**
     * Starts an immutable request builder with default query parameters.
     *
     * @param  array<string, mixed>  $query
     */
    public function withQuery(array $query): PendingRequest
    {
        return $this->newRequest()->withQuery($query);
    }

    /**
     * Starts an immutable request builder that form-encodes payloads.
     */
    public function asForm(): PendingRequest
    {
        return $this->newRequest()->asForm();
    }

    /**
     * Starts an immutable request builder in `multipart/form-data` mode.
     */
    public function asMultipart(): PendingRequest
    {
        return $this->newRequest()->asMultipart();
    }

    /**
     * Starts an immutable request builder with a file attached (multipart).
     *
     * @param  string|resource  $contents
     */
    public function attach(
        string $name,
        mixed $contents,
        ?string $filename = null,
        ?string $contentType = null,
    ): PendingRequest {
        return $this->newRequest()->attach($name, $contents, $filename, $contentType);
    }

    /**
     * Starts an immutable request builder with an `Accept` header.
     */
    public function accept(string $contentType): PendingRequest
    {
        return $this->newRequest()->accept($contentType);
    }

    /**
     * Starts an immutable request builder with `Accept: application/json`.
     */
    public function acceptJson(): PendingRequest
    {
        return $this->newRequest()->acceptJson();
    }

    /**
     * Sends a GET request.
     *
     * @param  array<string, mixed>  $query
     *
     * @throws RequestException On a transport failure (network, TLS, proxy, timeout).
     */
    public function get(string $url, array $query = []): Response
    {
        return $this->newRequest()->get($url, $query);
    }

    /**
     * Sends a HEAD request.
     *
     * @param  array<string, mixed>  $query
     *
     * @throws RequestException On a transport failure (network, TLS, proxy, timeout).
     */
    public function head(string $url, array $query = []): Response
    {
        return $this->newRequest()->head($url, $query);
    }

    /**
     * Sends a POST request.
     *
     * @param  array<string, mixed>|string  $data
     *
     * @throws RequestException On a transport failure (network, TLS, proxy, timeout).
     */
    public function post(string $url, array|string $data = []): Response
    {
        return $this->newRequest()->post($url, $data);
    }

    /**
     * Sends a PUT request.
     *
     * @param  array<string, mixed>|string  $data
     *
     * @throws RequestException On a transport failure (network, TLS, proxy, timeout).
     */
    public function put(string $url, array|string $data = []): Response
    {
        return $this->newRequest()->put($url, $data);
    }

    /**
     * Sends a PATCH request.
     *
     * @param  array<string, mixed>|string  $data
     *
     * @throws RequestException On a transport failure (network, TLS, proxy, timeout).
     */
    public function patch(string $url, array|string $data = []): Response
    {
        return $this->newRequest()->patch($url, $data);
    }

    /**
     * Sends a DELETE request.
     *
     * @param  array<string, mixed>|string  $data
     *
     * @throws RequestException On a transport failure (network, TLS, proxy, timeout).
     */
    public function delete(string $url, array|string $data = []): Response
    {
        return $this->newRequest()->delete($url, $data);
    }

    /**
     * Sends a request with an arbitrary HTTP method.
     *
     * @param  array<string, mixed>|string|null  $data
     *
     * @throws RequestException On a transport failure (network, TLS, proxy, timeout).
     */
    public function send(string $method, string $url, array|string|null $data = null): Response
    {
        return $this->newRequest()->send($method, $url, $data);
    }

    /**
     * Releases the connection pool now, closing every idle keep-alive socket.
     * Further requests through this client raise an exception.
     */
    public function close(): void
    {
        $this->ext->close();
    }

    /**
     * Whether the client is still usable (not yet closed).
     */
    public function isOpen(): bool
    {
        return $this->ext->isOpen();
    }

    /**
     * The release version of the loaded native `wreq_php` extension.
     */
    public static function extensionVersion(): string
    {
        Extension::ensure();

        return Ext\Client::extensionVersion();
    }

    /**
     * Creates a fresh per-request builder bound to this client's pool.
     */
    private function newRequest(): PendingRequest
    {
        return new PendingRequest($this->ext, $this->baseUrl);
    }
}
