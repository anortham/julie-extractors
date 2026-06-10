<?php

namespace Fixture;

class Worker
{
    public int $id;

    public function __construct(int $id)
    {
        $this->id = $id;
    }

    public function run(): int
    {
        recordRun($this->id);
        return helper($this->id);
    }
}

/**
 * Increment a worker id.
 *
 * @param int $value the worker id
 * @return int the incremented id
 */
function helper(int $value): int
{
    return $value + 1;
}

/** Emits a worker-run marker for observability hooks. */
function recordRun(int $id): void
{
    observeRun("worker-run", $id);
}

/** Records a named worker event for downstream hooks. */
function observeRun(string $event, int $id): void
{
}

/** Checks the worker service health endpoint. */
function fetchStatus(): void
{
    fetchUrl("https://api.example.com/workers/status");
}

function fetchUrl(string $url): void
{
}
