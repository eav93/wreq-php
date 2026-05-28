<?php

/**
 * Minimal single-threaded HTTP/1.1 server with keep-alive, used by the
 * integration suite to prove the wreq-php connection pool actually reuses one
 * TCP socket across two requests.
 *
 * Usage:
 *   php keepalive_server.php <accept_log_path>
 *
 * The chosen port is printed to stdout on the first line so the parent test
 * can read it. Each accepted TCP connection increments a counter persisted to
 * the log path. The server exits on its own after a short deadline so a
 * runaway test cannot leave it behind.
 */

declare(strict_types=1);

$logPath = $argv[1] ?? null;
if ($logPath === null) {
    fwrite(STDERR, "usage: keepalive_server.php <log_path>\n");
    exit(1);
}

$server = @stream_socket_server('tcp://127.0.0.1:0', $errno, $errstr);
if ($server === false) {
    fwrite(STDERR, "bind failed: {$errstr}\n");
    exit(1);
}

$name = stream_socket_get_name($server, false);
if ($name === false) {
    fwrite(STDERR, "stream_socket_get_name failed\n");
    exit(1);
}
$port = (int) substr($name, (int) strrpos($name, ':') + 1);
echo $port."\n";
@fflush(STDOUT);

file_put_contents($logPath, '0');

// Server accepts connections sequentially. A 5-second per-accept timeout
// caps how long it waits between client connections, so a test that aborts
// early does not leave a stuck process behind. Total lifetime is bounded by
// the outer deadline.
$accepts = 0;
$deadline = microtime(true) + 10.0;

while (microtime(true) < $deadline) {
    $remaining = $deadline - microtime(true);
    if ($remaining <= 0) {
        break;
    }
    $client = @stream_socket_accept($server, min(1.0, $remaining));
    if ($client === false) {
        continue;
    }

    $accepts++;
    file_put_contents($logPath, (string) $accepts);
    handle_connection($client);
    @fclose($client);
}

@fclose($server);

function handle_connection($client): void
{
    stream_set_timeout($client, 10);

    while (! feof($client)) {
        $request = '';
        while (($line = fgets($client)) !== false) {
            $request .= $line;
            if (rtrim($line, "\r\n") === '') {
                break;
            }
        }
        if ($request === '') {
            return;
        }

        if (preg_match('/Content-Length:\s*(\d+)/i', $request, $m) === 1) {
            $remaining = (int) $m[1];
            while ($remaining > 0) {
                $chunk = fread($client, min($remaining, 8192));
                if ($chunk === false || $chunk === '') {
                    return;
                }
                $remaining -= strlen($chunk);
            }
        }

        $body = 'ok';
        $response = "HTTP/1.1 200 OK\r\n"
            ."Content-Length: ".strlen($body)."\r\n"
            ."Connection: keep-alive\r\n"
            ."Content-Type: text/plain\r\n"
            ."\r\n"
            .$body;
        @fwrite($client, $response);
    }
}
