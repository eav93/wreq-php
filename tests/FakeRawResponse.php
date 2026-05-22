<?php

declare(strict_types=1);

namespace Wreq\Tests;

/**
 * Test double mimicking the native `Wreq\Ext\Response`.
 */
final class FakeRawResponse
{
    /**
     * @param  array<string, array<int, string>>  $headers
     */
    public function __construct(
        private readonly int $status = 200,
        private readonly string $body = '',
        private readonly array $headers = [],
        private readonly string $version = 'HTTP/2.0',
        private readonly string $url = 'https://example.test/',
        private readonly ?string $remoteAddr = null,
    ) {}

    public function status(): int
    {
        return $this->status;
    }

    public function body(): string
    {
        return $this->body;
    }

    public function version(): string
    {
        return $this->version;
    }

    public function url(): string
    {
        return $this->url;
    }

    public function remoteAddr(): ?string
    {
        return $this->remoteAddr;
    }

    /**
     * @return array<string, array<int, string>>
     */
    public function headers(): array
    {
        return $this->headers;
    }

    public function header(string $name): ?string
    {
        $values = $this->headers[strtolower($name)] ?? null;

        return $values === null ? null : implode(', ', $values);
    }
}
