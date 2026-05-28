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
 * Production must enable it in `php.ini`:  `extension=/path/to/wreq_php.so`.
 *
 * For CLI we additionally try `dl('wreq_php.{so,dll}')` — `dl()` always
 * resolves names against `extension_dir` regardless of the path passed, so it
 * only works when the operator already placed the binary there under the
 * conventional filename. The Composer-fetched binary in `vendor/.../runtime/`
 * is not in `extension_dir` and will not be picked up by `dl()`; the helpful
 * error message below tells the user how to wire it up instead.
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
     * Tries `dl('wreq_php.{so,dll}')`. PHP strips any path and looks in
     * `extension_dir` regardless — so this succeeds *only* when the operator
     * has placed the binary there manually under the conventional filename
     * (and `enable_dl=On` is set, which is CLI-only on most builds).
     *
     * The Composer-fetched binary in `vendor/.../runtime/` is intentionally
     * NOT auto-copied here: writing into `extension_dir` typically needs root.
     */
    private static function tryLoad(): bool
    {
        if (! function_exists('dl')) {
            return false;
        }

        $candidate = self::NAME.(PHP_OS_FAMILY === 'Windows' ? '.dll' : '.so');
        $loaded = @dl($candidate);

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
