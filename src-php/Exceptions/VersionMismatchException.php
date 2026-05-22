<?php

declare(strict_types=1);

namespace Wreq\Exceptions;

/**
 * Thrown when the native `wreq_php` binary and the Composer package come from
 * incompatible releases (their major.minor versions differ).
 */
final class VersionMismatchException extends \RuntimeException {}
