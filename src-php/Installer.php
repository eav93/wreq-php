<?php

declare(strict_types=1);

namespace Wreq;

/**
 * Composer post-install hook that fetches a prebuilt native binary.
 *
 * The `wreq_php` extension is platform-specific (PHP version × NTS/ZTS × OS ×
 * arch). CI publishes one binary per combination — plus a `.sha256` companion —
 * to the GitHub Release that matches the installed package version. This script
 * downloads the matching pair into `runtime/`, verifies the checksum, and writes
 * the file atomically.
 *
 * It never fails the Composer run: on any problem it prints build-from-source
 * instructions and exits cleanly. Set `WREQ_PHP_BINARY` to a local binary and
 * downloading is skipped entirely.
 */
final class Installer
{
    /** GitHub `owner/repo` hosting the release binaries. */
    private const REPO = 'eav93/wreq-php';

    /** Composer package name, used to resolve the installed version. */
    private const PACKAGE = 'eav93/wreq-php';

    /** Reject a "binary" smaller than this — it is almost certainly an error page. */
    private const MIN_BINARY_BYTES = 4096;

    /**
     * Composer entry point.
     */
    public static function run(): void
    {
        try {
            self::install();
        } catch (\Throwable $e) {
            self::line('wreq-php: skipping prebuilt binary ('.$e->getMessage().').');
            self::line('wreq-php: build it yourself with `cargo build --release`.');
        }
    }

    private static function install(): void
    {
        if (extension_loaded('wreq_php')) {
            return;
        }

        $override = getenv('WREQ_PHP_BINARY');
        if (is_string($override) && $override !== '') {
            self::line("wreq-php: using WREQ_PHP_BINARY at {$override}.");

            return;
        }

        $target = self::target();
        $runtimeDir = \dirname(__DIR__).'/runtime';
        $dest = $runtimeDir.'/'.$target;

        if (is_file($dest)) {
            self::announce($dest);

            return;
        }

        if (! is_dir($runtimeDir) && ! mkdir($runtimeDir, 0o755, true) && ! is_dir($runtimeDir)) {
            throw new \RuntimeException("cannot create {$runtimeDir}");
        }

        $base = self::releaseBase();
        self::line("wreq-php: downloading {$base}/{$target}");

        // Fetch the checksum first; a missing checksum means we will not trust
        // the binary at all.
        $expected = self::parseChecksum(self::download("{$base}/{$target}.sha256"));
        if ($expected === null) {
            throw new \RuntimeException("no checksum published for {$target}");
        }

        $binary = self::download("{$base}/{$target}");
        if (\strlen($binary) < self::MIN_BINARY_BYTES) {
            throw new \RuntimeException("downloaded file for {$target} is implausibly small");
        }

        $actual = hash('sha256', $binary);
        if (! hash_equals($expected, $actual)) {
            throw new \RuntimeException("checksum mismatch for {$target}");
        }

        self::writeAtomically($dest, $binary);
        self::announce($dest);
    }

    /**
     * Computes the release asset name for the current environment.
     */
    public static function target(): string
    {
        $php = PHP_MAJOR_VERSION.'.'.PHP_MINOR_VERSION;
        $threadSafety = PHP_ZTS ? 'zts' : 'nts';

        $os = match (PHP_OS_FAMILY) {
            'Windows' => 'windows',
            'Darwin' => 'macos',
            default => 'linux',
        };
        $extension = $os === 'windows' ? 'dll' : 'so';

        $machine = strtolower(php_uname('m'));
        $arch = match ($machine) {
            'arm64', 'aarch64' => 'aarch64',
            'x86_64', 'amd64' => 'x86_64',
            default => $machine,
        };

        return "wreq_php-php{$php}-{$threadSafety}-{$os}-{$arch}.{$extension}";
    }

    /**
     * Resolves the release base URL for the installed package version.
     *
     * A released version maps to its tagged release; a dev/unknown version
     * falls back to the latest release as a best effort.
     */
    private static function releaseBase(): string
    {
        $version = self::packageVersion();

        if ($version !== null && preg_match('/^v?(\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?)$/', $version, $m)) {
            return 'https://github.com/'.self::REPO.'/releases/download/v'.$m[1];
        }

        return 'https://github.com/'.self::REPO.'/releases/latest/download';
    }

    /**
     * The installed package version, if Composer can report it.
     */
    private static function packageVersion(): ?string
    {
        if (! class_exists(\Composer\InstalledVersions::class)) {
            return null;
        }

        try {
            return \Composer\InstalledVersions::getPrettyVersion(self::PACKAGE);
        } catch (\Throwable) {
            return null;
        }
    }

    /**
     * Downloads a URL, following redirects and rejecting non-2xx responses so an
     * HTML error page is never mistaken for a binary.
     */
    private static function download(string $url): string
    {
        $context = stream_context_create([
            'http' => [
                'method' => 'GET',
                'follow_location' => 1,
                'max_redirects' => 5,
                'timeout' => 120,
                'ignore_errors' => true,
                'user_agent' => 'wreq-php-installer',
            ],
        ]);

        $body = @file_get_contents($url, false, $context);
        if ($body === false) {
            throw new \RuntimeException("request failed: {$url}");
        }

        // PHP 8.5 deprecated the magic `$http_response_header` variable in
        // favour of http_get_last_response_headers(); use whichever exists.
        if (function_exists('http_get_last_response_headers')) {
            $headers = http_get_last_response_headers() ?? [];
        } else {
            /** @var array<int, string> $http_response_header */
            $headers = $http_response_header ?? [];
        }

        $status = self::statusCode($headers);
        if ($status < 200 || $status >= 300) {
            throw new \RuntimeException("HTTP {$status} for {$url}");
        }

        return $body;
    }

    /**
     * Extracts the final HTTP status code from a response header list.
     *
     * @param  array<int, string>  $headers
     */
    private static function statusCode(array $headers): int
    {
        $status = 0;
        foreach ($headers as $header) {
            if (preg_match('#^HTTP/\S+\s+(\d{3})#', $header, $m)) {
                $status = (int) $m[1];
            }
        }

        return $status;
    }

    /**
     * Extracts the hex digest from a `shasum`-style `<hash>  <file>` line.
     */
    private static function parseChecksum(string $contents): ?string
    {
        return preg_match('/\b([a-f0-9]{64})\b/i', $contents, $m) ? strtolower($m[1]) : null;
    }

    /**
     * Writes the binary to its final path via a temp file + rename.
     */
    private static function writeAtomically(string $dest, string $binary): void
    {
        $tmp = $dest.'.'.bin2hex(random_bytes(6)).'.tmp';

        if (file_put_contents($tmp, $binary) === false) {
            throw new \RuntimeException("cannot write {$tmp}");
        }
        @chmod($tmp, 0o644);

        if (! rename($tmp, $dest)) {
            @unlink($tmp);
            throw new \RuntimeException("cannot finalize {$dest}");
        }
    }

    private static function announce(string $path): void
    {
        self::line("wreq-php: native binary ready at {$path}");
        self::line("wreq-php: enable it by adding to php.ini  =>  extension={$path}");
    }

    private static function line(string $message): void
    {
        fwrite(STDERR, $message."\n");
    }
}
