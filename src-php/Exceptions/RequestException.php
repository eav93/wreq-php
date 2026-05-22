<?php

declare(strict_types=1);

namespace Wreq\Exceptions;

use Wreq\Response;

/**
 * Thrown by {@see Response::throw()} when a response has a 4xx/5xx status.
 *
 * The originating {@see Response} stays reachable via {@see self::response()}
 * so callers can still inspect the body, headers and status.
 */
final class RequestException extends \RuntimeException
{
    public function __construct(private readonly Response $response)
    {
        parent::__construct(sprintf(
            'HTTP request returned status %d for %s',
            $response->status(),
            $response->url(),
        ));
    }

    /**
     * The response that triggered this exception.
     */
    public function response(): Response
    {
        return $this->response;
    }
}
