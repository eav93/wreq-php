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

    public function __construct(private readonly FakeRawResponse $response = new FakeRawResponse())
    {
    }

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

    public function close(): void
    {
    }

    public function isOpen(): bool
    {
        return true;
    }
}
