<?php

declare(strict_types=1);

namespace Wreq\Support;

use Composer\InstalledVersions;
use Wreq\Exceptions\ExtensionMissingException;
use Wreq\Exceptions\VersionMismatchException;
use Wreq\Ext\Client;

/**
 * Locates and loads the native `wreq_php` extension.
 *
 * A PHP extension must normally be enabled in `php.ini`. For CLI/dev use we
 * also attempt a runtime `dl()` of the binary fetched by the Composer
 * installer. Production should add `extension=...` to `php.ini`.
 *
 * @internal
 */
final class Extension
{
    public const NAME = 'wreq_php';

    private static bool $ensured = false;

    /**
     * Makes sure the extension is loaded, or throws with guidance.
     */
    public static function ensure(): void
    {
        if (self::$ensured) {
            return;
        }

        if (extension_loaded(self::NAME)) {
            self::checkVersion();
            self::$ensured = true;

            return;
        }

        if (self::tryLoad()) {
            self::checkVersion();
            self::$ensured = true;

            return;
        }

        throw new ExtensionMissingException(
            "The native '".self::NAME."' extension is not loaded.\n".
            'Add it to your php.ini:  extension='.(self::binaryPath() ?? '/path/to/'.self::NAME.'.so')."\n".
            'Or run `composer install` to fetch a prebuilt binary, '.
            'or build it with `cargo build --release`.'
        );
    }

    /**
     * Attempts to `dl()` the binary (only works on CLI with `enable_dl=On`).
     */
    private static function tryLoad(): bool
    {
        $path = self::binaryPath();

        if ($path === null || ! function_exists('dl')) {
            return false;
        }

        // `dl()` resolves names against `extension_dir`; an absolute path only
        // works when its directory matches. We still try by basename.
        $loaded = @dl(basename($path));

        return $loaded && extension_loaded(self::NAME);
    }

    /**
     * Verifies the native binary and the Composer package are compatible — i.e.
     * built from the same major.minor release. A mismatch is an easy footgun
     * when the `.so` is baked into a Docker image separately from the
     * `composer install` of the PHP layer.
     *
     * Best-effort: skipped for local/dev builds and whenever either version
     * cannot be determined, so it never produces a false alarm.
     */
    private static function checkVersion(): void
    {
        if (! method_exists('Wreq\Ext\Client', 'extensionVersion')) {
            return; // an older binary that does not report its version
        }

        $binary = Client::extensionVersion();
        $package = self::packageVersion();

        $binaryLine = self::majorMinor($binary);
        $packageLine = self::majorMinor($package);

        if ($binaryLine === null || $packageLine === null || $binaryLine === $packageLine) {
            return;
        }

        throw new VersionMismatchException(sprintf(
            'wreq_php version mismatch: the native extension is %s but the '.
            "Composer package is %s.\nRebuild so both come from the same ".
            'release — within a 0.x line the major.minor must match.',
            $binary,
            $package ?? 'unknown',
        ));
    }

    /**
     * The installed Composer package version, if Composer can report it.
     */
    private static function packageVersion(): ?string
    {
        if (! class_exists(InstalledVersions::class)) {
            return null;
        }

        try {
            return InstalledVersions::getPrettyVersion('eav93/wreq-php');
        } catch (\Throwable) {
            return null;
        }
    }

    /**
     * Extracts `MAJOR.MINOR` from a version string; null for a dev/unknown one
     * (those are never treated as a mismatch).
     */
    private static function majorMinor(?string $version): ?string
    {
        if ($version === null || str_contains($version, 'dev')) {
            return null;
        }

        return preg_match('/(\d+)\.(\d+)/', $version, $m) ? $m[1].'.'.$m[2] : null;
    }

    /**
     * Resolves the prebuilt binary path, if one is available.
     */
    public static function binaryPath(): ?string
    {
        $override = getenv('WREQ_PHP_BINARY');
        if (is_string($override) && $override !== '' && is_file($override)) {
            return $override;
        }

        $runtime = \dirname(__DIR__, 2).'/runtime';
        if (! is_dir($runtime)) {
            return null;
        }

        foreach (glob($runtime.'/'.self::NAME.'*') ?: [] as $candidate) {
            if (is_file($candidate)) {
                return $candidate;
            }
        }

        return null;
    }
}
