<?php

namespace Fixture;

#[\Attribute(\Attribute::TARGET_CLASS | \Attribute::TARGET_METHOD | \Attribute::TARGET_PROPERTY)]
class Entity
{
}

#[\Attribute(\Attribute::TARGET_METHOD)]
class Route
{
    public function __construct(public string $path)
    {
    }
}

#[\Attribute(\Attribute::TARGET_PROPERTY)]
class Required
{
}

#[Entity]
class Worker
{
    #[Required]
    public int $id;

    public function __construct(int $id)
    {
        $this->id = $id;
    }

    #[Route('/run')]
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

function evaluate(int $count, bool $enabled): int
{
    $total = 0;
    if ($enabled) {
        for ($i = 0; $i < $count; $i++) {
            $total += $i;
        }
    } elseif ($count > 0) {
        $total = $count > 10 ? 1 : 0;
    }
    return $total;
}
