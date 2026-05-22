<?php

declare(strict_types=1);

namespace Wreq\Support;

use Wreq\Exceptions\ExtensionMissingException;

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
            self::$ensured = true;

            return;
        }

        if (self::tryLoad()) {
            self::$ensured = true;

            return;
        }

        throw new ExtensionMissingException(
            "The native '".self::NAME."' extension is not loaded.\n".
            "Add it to your php.ini:  extension=".(self::binaryPath() ?? '/path/to/'.self::NAME.'.so')."\n".
            "Or run `composer install` to fetch a prebuilt binary, ".
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
