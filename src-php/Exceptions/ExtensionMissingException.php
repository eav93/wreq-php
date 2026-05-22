<?php

declare(strict_types=1);

namespace Wreq\Exceptions;

/**
 * Thrown when the native `wreq_php` extension cannot be found or loaded.
 */
final class ExtensionMissingException extends \RuntimeException
{
}
