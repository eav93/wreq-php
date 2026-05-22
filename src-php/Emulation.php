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

        return \Wreq\Ext\Emulation::all();
    }

    /**
     * A random profile name.
     */
    public static function random(): string
    {
        Extension::ensure();

        return \Wreq\Ext\Emulation::random();
    }

    /**
     * Whether the given profile name is recognized.
     */
    public static function exists(string $name): bool
    {
        Extension::ensure();

        return \Wreq\Ext\Emulation::exists($name);
    }
}
