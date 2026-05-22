<?php

declare(strict_types=1);

namespace Wreq;

use Wreq\Support\Extension;

/**
 * Browser emulation profile registry.
 *
 * Profiles come straight from the `wreq-util` crate, so upgrading the native
 * extension exposes new browsers automatically — no PHP changes needed. Pass a
 * profile name to {@see Client} via the `emulation` option.
 *
 * ```php
 * $client = new Wreq\Client(['emulation' => Wreq\Emulation::random()]);
 * ```
 */
final class Emulation
{
    /**
     * Every supported profile name (e.g. `chrome_131`, `firefox_136`).
     *
     * @return array<int, string>
     */
    public static function all(): array
    {
        Extension::ensure();

        return Ext\Emulation::all();
    }

    /**
     * A random profile name.
     *
     * With `$like` set, the choice is restricted to profiles whose name starts
     * with it — e.g. `random('chrome')` picks a random Chrome version. Pair it
     * with the `os` emulation option to fix the platform:
     *
     * ```php
     * new Wreq\Client(['emulation' => [
     *     'profile' => Wreq\Emulation::random('chrome'),
     *     'os'      => 'windows',
     * ]]);
     * ```
     */
    public static function random(?string $like = null): string
    {
        Extension::ensure();

        if ($like === null) {
            return Ext\Emulation::random();
        }

        $matches = self::like($like);
        if ($matches === []) {
            throw new \InvalidArgumentException("no emulation profile matches '{$like}'");
        }

        return $matches[array_rand($matches)];
    }

    /**
     * Every profile name starting with the given prefix (e.g. `chrome`).
     *
     * @return array<int, string>
     */
    public static function like(string $prefix): array
    {
        return array_values(array_filter(
            self::all(),
            static fn (string $name): bool => str_starts_with($name, $prefix),
        ));
    }

    /**
     * Whether the given profile name is recognized.
     */
    public static function exists(string $name): bool
    {
        Extension::ensure();

        return Ext\Emulation::exists($name);
    }
}
