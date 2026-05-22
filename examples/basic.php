<?php

declare(strict_types=1);

/**
 * Basic usage of wreq-php.
 *
 * Run with the extension loaded, e.g.:
 *   php -d extension=./target/release/libwreq_php.so examples/basic.php
 */

require __DIR__.'/../vendor/autoload.php';

use Wreq\Client;
use Wreq\Emulation;

// One reusable client === one connection pool. Keep it around and every
// request below reuses the same keep-alive TCP/TLS connections.
$client = new Client([
    'emulation' => 'chrome_131', // browser TLS/HTTP2 fingerprint
    'pool_max_idle_per_host' => 8,            // TCP connections kept per host
    'cookies' => true,         // shared cookie jar for this client
    'timeout' => 30.0,
]);

// GET with query parameters.
$response = $client->get('https://httpbin.org/get', ['q' => 'wreq']);

echo 'Status:  ', $response->status(), "\n";
echo 'OK:      ', $response->successful() ? 'yes' : 'no', "\n";
echo 'Args:    ', json_encode($response->json('args')), "\n";
echo 'Server:  ', $response->header('server'), "\n";

// POST JSON (the default body format).
$created = $client->post('https://httpbin.org/post', [
    'name' => 'Ada',
    'role' => 'engineer',
]);
echo 'Echoed:  ', json_encode($created->json('json')), "\n";

// POST form-encoded — per-request tweak via the immutable builder.
$client->asForm()->post('https://httpbin.org/post', ['field' => 'value']);

// Bearer token for a single request; the base client is untouched.
$client->withToken('secret-token')->get('https://httpbin.org/bearer');

// A few available emulation profiles.
echo 'Profiles: ', implode(', ', array_slice(Emulation::all(), 0, 5)), " ...\n";

// Release the pool and close every idle socket right now.
$client->close();
