<?php

declare(strict_types=1);

namespace Wreq\Tests;

use PHPUnit\Framework\TestCase;
use Wreq\Exceptions\RequestException;
use Wreq\Response;

final class ResponseTest extends TestCase
{
    public function test_body_and_status(): void
    {
        $response = new Response(new FakeRawResponse(status: 201, body: 'hello'));

        $this->assertSame(201, $response->status());
        $this->assertSame('hello', $response->body());
    }

    public function test_json_whole_and_dot_notation(): void
    {
        $payload = json_encode(['data' => ['user' => ['name' => 'Ada']], 'count' => 2]);
        $response = new Response(new FakeRawResponse(body: $payload));

        $this->assertSame(2, $response->json('count'));
        $this->assertSame('Ada', $response->json('data.user.name'));
        $this->assertSame('fallback', $response->json('data.user.missing', 'fallback'));
        $this->assertSame(['user' => ['name' => 'Ada']], $response->json('data'));
    }

    public function test_object_returns_object_graph(): void
    {
        $response = new Response(new FakeRawResponse(body: '{"x":1}'));

        $this->assertIsObject($response->object());
        $this->assertSame(1, $response->object()->x);
    }

    public function test_json_throws_on_malformed_body(): void
    {
        $response = new Response(new FakeRawResponse(body: 'not json at all'));

        $this->expectException(\JsonException::class);
        $response->json();
    }

    public function test_json_distinguishes_literal_null_from_missing(): void
    {
        $response = new Response(new FakeRawResponse(body: 'null'));

        $this->assertNull($response->json());
        $this->assertSame('fallback', $response->json('missing.key', 'fallback'));
    }

    public function test_resource_streams_the_body(): void
    {
        $response = new Response(new FakeRawResponse(body: 'streamed'));
        $resource = $response->resource();

        $this->assertIsResource($resource);
        $this->assertSame('streamed', stream_get_contents($resource));
        fclose($resource);
    }

    public function test_status_helpers(): void
    {
        $this->assertTrue((new Response(new FakeRawResponse(status: 200)))->successful());
        $this->assertTrue((new Response(new FakeRawResponse(status: 301)))->redirect());
        $this->assertTrue((new Response(new FakeRawResponse(status: 404)))->clientError());
        $this->assertTrue((new Response(new FakeRawResponse(status: 500)))->serverError());
        $this->assertTrue((new Response(new FakeRawResponse(status: 503)))->failed());
        $this->assertFalse((new Response(new FakeRawResponse(status: 204)))->failed());
    }

    public function test_headers(): void
    {
        $response = new Response(new FakeRawResponse(headers: [
            'content-type' => ['application/json'],
            'set-cookie' => ['a=1', 'b=2'],
        ]));

        $this->assertSame('application/json', $response->header('Content-Type'));
        $this->assertSame('a=1, b=2', $response->header('set-cookie'));
        $this->assertSame('', $response->header('x-absent'));
        $this->assertArrayHasKey('set-cookie', $response->headers());
    }

    public function test_throw_on_failure(): void
    {
        $ok = new Response(new FakeRawResponse(status: 200));
        $this->assertSame($ok, $ok->throw());

        $this->expectException(RequestException::class);
        (new Response(new FakeRawResponse(status: 500)))->throw();
    }

    public function test_sink_metadata_is_exposed(): void
    {
        $raw = new FakeRawResponse(status: 200, body: '', downloadedBytes: 4096);
        $response = new Response($raw, '/tmp/out.bin');

        $this->assertSame('/tmp/out.bin', $response->savedTo());
        $this->assertSame(4096, $response->downloadedBytes());
    }

    public function test_in_memory_response_has_no_sink_metadata(): void
    {
        $response = new Response(new FakeRawResponse(body: 'hello'));

        $this->assertNull($response->savedTo());
        $this->assertNull($response->downloadedBytes());
    }
}
