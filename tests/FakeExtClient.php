<?php

declare(strict_types=1);

namespace Wreq\Tests;

/**
 * Test double mimicking the native `Wreq\Ext\Client`. Records the last request
 * so tests can assert on URL/header/body building.
 */
final class FakeExtClient
{
    /** @var array{method: string, url: string, headers: array<string,string>|null, body: string|null}|null */
    public ?array $lastRequest = null;

    /** @var array{method: string, url: string, headers: mixed, fields: mixed, files: mixed}|null */
    public ?array $lastMultipart = null;

    /** @var array{method: string, url: string, headers: mixed, body: string|null, path: string}|null */
    public ?array $lastDownload = null;

    public function __construct(private readonly FakeRawResponse $response = new FakeRawResponse) {}

    /**
     * @param  array<string, string>|null  $headers
     */
    public function request(string $method, string $url, ?array $headers = null, ?string $body = null): FakeRawResponse
    {
        $this->lastRequest = [
            'method' => $method,
            'url' => $url,
            'headers' => $headers,
            'body' => $body,
        ];

        return $this->response;
    }

    /**
     * @param  array<string, string>|null  $headers
     * @param  array<string, mixed>|null  $fields
     * @param  array<int, array<string, string>>|null  $files
     */
    public function requestMultipart(
        string $method,
        string $url,
        ?array $headers = null,
        ?array $fields = null,
        ?array $files = null,
    ): FakeRawResponse {
        $this->lastMultipart = [
            'method' => $method,
            'url' => $url,
            'headers' => $headers,
            'fields' => $fields,
            'files' => $files,
        ];

        return $this->response;
    }

    /**
     * @param  array<string, string>|null  $headers
     */
    public function requestToFile(
        string $method,
        string $url,
        ?array $headers = null,
        ?string $body = null,
        string $path = '',
    ): FakeRawResponse {
        $this->lastDownload = [
            'method' => $method,
            'url' => $url,
            'headers' => $headers,
            'body' => $body,
            'path' => $path,
        ];

        return $this->response;
    }

    public function close(): void {}

    public function isOpen(): bool
    {
        return true;
    }
}
