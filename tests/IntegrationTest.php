<?php

declare(strict_types=1);

namespace Wreq\Tests;

use PHPUnit\Framework\Attributes\Group;
use PHPUnit\Framework\TestCase;
use Wreq\Client;
use Wreq\Emulation;
use Wreq\Response;

/**
 * End-to-end tests against the real native extension and network.
 *
 * Skipped automatically when the `wreq_php` extension is not loaded, so the
 * pure-PHP suite still runs everywhere.
 */
#[Group('integration')]
final class IntegrationTest extends TestCase
{
    private string $httpbin;

    protected function setUp(): void
    {
        if (! extension_loaded('wreq_php')) {
            $this->markTestSkipped('the wreq_php extension is not loaded');
        }

        $this->httpbin = getenv('WREQ_HTTPBIN') ?: 'https://httpbin.org';
    }

    public function test_get_returns_decoded_json(): void
    {
        $client = new Client(['timeout' => 30.0]);
        $response = $client->get($this->httpbin.'/get', ['hello' => 'world']);
        $this->skipIfHttpbinUnavailable($response);

        $this->assertSame(200, $response->status());
        $this->assertSame('world', $response->json('args.hello'));
    }

    public function test_post_json_round_trip(): void
    {
        $client = new Client(['timeout' => 30.0]);
        $response = $client->post($this->httpbin.'/post', ['name' => 'Ada']);
        $this->skipIfHttpbinUnavailable($response);

        $this->assertSame(200, $response->status());
        $this->assertSame('Ada', $response->json('json.name'));
    }

    public function test_emulation_profiles_are_available(): void
    {
        $profiles = Emulation::all();

        $this->assertNotEmpty($profiles);
        $this->assertTrue(Emulation::exists($profiles[0]));
        $this->assertFalse(Emulation::exists('not_a_browser_999'));
    }

    public function test_random_emulation_filtered_by_family(): void
    {
        $chrome = Emulation::random('chrome');

        $this->assertStringStartsWith('chrome', $chrome);
        $this->assertTrue(Emulation::exists($chrome));
        $this->assertNotEmpty(Emulation::like('chrome'));

        $this->expectException(\InvalidArgumentException::class);
        Emulation::random('not_a_browser');
    }

    public function test_connection_pool_is_reused_within_a_client(): void
    {
        $client = new Client(['timeout' => 30.0]);

        // Two requests to the same host should reuse one keep-alive socket,
        // so the remote peer address is identical.
        $first = $client->get($this->httpbin.'/get');
        $second = $client->get($this->httpbin.'/get');

        $this->assertSame($first->remoteAddr(), $second->remoteAddr());
    }

    public function test_close_releases_the_client(): void
    {
        $client = new Client(['timeout' => 30.0]);
        $this->assertTrue($client->isOpen());

        $client->close();
        $this->assertFalse($client->isOpen());

        $this->expectException(\Throwable::class);
        $client->get($this->httpbin.'/get');
    }

    public function test_invalid_emulation_profile_is_rejected(): void
    {
        $this->expectException(\Throwable::class);
        new Client(['emulation' => 'definitely_not_a_browser']);
    }

    public function test_non_finite_timeout_is_rejected(): void
    {
        $this->expectException(\Throwable::class);
        new Client(['pool_idle_timeout' => NAN]);
    }

    public function test_full_option_set_is_accepted(): void
    {
        $client = new Client([
            'emulation' => 'firefox_136',
            'gzip' => true,
            'brotli' => true,
            'http2_only' => true,
            'tcp_nodelay' => true,
            'tcp_keepalive' => 30.0,
            'tcp_keepalive_retries' => 3,
            'tcp_recv_buffer_size' => 65536,
            'min_tls_version' => '1.2',
            'referer' => true,
            'connect_timeout' => 10.0,
            'timeout' => 30.0,
        ]);

        $this->assertTrue($client->isOpen());
    }

    public function test_cookie_jar_persists_across_requests(): void
    {
        $client = new Client(['cookies' => true, 'timeout' => 30.0]);

        $client->get($this->httpbin.'/cookies/set', ['session' => 'abc']);
        $response = $client->get($this->httpbin.'/cookies');
        $this->skipIfHttpbinUnavailable($response);

        $this->assertSame('abc', $response->json('cookies.session'));
    }

    public function test_extension_reports_its_version(): void
    {
        $this->assertMatchesRegularExpression('/^\d+\.\d+\.\d+/', Client::extensionVersion());
    }

    public function test_multipart_upload(): void
    {
        $client = new Client(['timeout' => 30.0]);
        $response = $client
            ->attach('document', 'file-content-here', 'doc.txt', 'text/plain')
            ->post($this->httpbin.'/post', ['caption' => 'hello']);
        $this->skipIfHttpbinUnavailable($response);

        $this->assertSame('hello', $response->json('form.caption'));
        $this->assertSame('file-content-here', $response->json('files.document'));
    }

    /**
     * httpbin.org is a free shared service that intermittently returns 5xx or
     * non-JSON error pages. When that happens, skip — these tests exercise the
     * extension, not httpbin's uptime.
     */
    private function skipIfHttpbinUnavailable(Response $response): void
    {
        if (! $response->successful()) {
            $this->markTestSkipped('httpbin returned HTTP '.$response->status());
        }
    }
}
