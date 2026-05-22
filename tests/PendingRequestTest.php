<?php

declare(strict_types=1);

namespace Wreq\Tests;

use PHPUnit\Framework\TestCase;
use Wreq\PendingRequest;

final class PendingRequestTest extends TestCase
{
    public function test_get_appends_query_string(): void
    {
        $ext = new FakeExtClient();
        (new PendingRequest($ext))->get('https://api.test/users', ['page' => 2, 'q' => 'a b']);

        $this->assertSame('GET', $ext->lastRequest['method']);
        $this->assertSame('https://api.test/users?page=2&q=a+b', $ext->lastRequest['url']);
        $this->assertNull($ext->lastRequest['body']);
    }

    public function test_base_url_is_applied_to_relative_paths(): void
    {
        $ext = new FakeExtClient();
        (new PendingRequest($ext, 'https://api.test/v1'))->get('/users');

        $this->assertSame('https://api.test/v1/users', $ext->lastRequest['url']);
    }

    public function test_base_url_is_ignored_for_absolute_urls(): void
    {
        $ext = new FakeExtClient();
        (new PendingRequest($ext, 'https://api.test'))->get('https://other.test/x');

        $this->assertSame('https://other.test/x', $ext->lastRequest['url']);
    }

    public function test_post_encodes_json_by_default(): void
    {
        $ext = new FakeExtClient();
        (new PendingRequest($ext))->post('https://api.test/users', ['name' => 'Ada']);

        $this->assertSame('POST', $ext->lastRequest['method']);
        $this->assertSame('{"name":"Ada"}', $ext->lastRequest['body']);
        $this->assertSame('application/json', $ext->lastRequest['headers']['Content-Type']);
    }

    public function test_as_form_encodes_form_body(): void
    {
        $ext = new FakeExtClient();
        (new PendingRequest($ext))->asForm()->post('https://api.test/users', ['name' => 'Ada']);

        $this->assertSame('name=Ada', $ext->lastRequest['body']);
        $this->assertSame(
            'application/x-www-form-urlencoded',
            $ext->lastRequest['headers']['Content-Type'],
        );
    }

    public function test_with_token_sets_authorization_header(): void
    {
        $ext = new FakeExtClient();
        (new PendingRequest($ext))->withToken('abc123')->get('https://api.test/me');

        $this->assertSame('Bearer abc123', $ext->lastRequest['headers']['Authorization']);
    }

    public function test_builder_is_immutable(): void
    {
        $ext = new FakeExtClient();
        $base = new PendingRequest($ext);
        $withHeader = $base->withHeader('X-Trace', '1');

        $this->assertNotSame($base, $withHeader);

        $base->get('https://api.test/a');
        $this->assertSame([], $ext->lastRequest['headers']);

        $withHeader->get('https://api.test/b');
        $this->assertSame('1', $ext->lastRequest['headers']['X-Trace']);
    }

    public function test_with_body_sends_raw_payload(): void
    {
        $ext = new FakeExtClient();
        (new PendingRequest($ext))
            ->withBody('<xml/>', 'application/xml')
            ->post('https://api.test/feed');

        $this->assertSame('<xml/>', $ext->lastRequest['body']);
        $this->assertSame('application/xml', $ext->lastRequest['headers']['Content-Type']);
    }
}
