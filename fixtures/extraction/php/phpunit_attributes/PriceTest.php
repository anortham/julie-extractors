<?php

namespace Tests\Unit;

use PHPUnit\Framework\Attributes\DataProvider;
use PHPUnit\Framework\Attributes\Group;
use PHPUnit\Framework\Attributes\Test;
use PHPUnit\Framework\TestCase;

final class PriceTest extends TestCase
{
    #[Group('slow')]
    #[Test]
    public function computes(): void
    {
        self::assertTrue(true);
    }

    #[Test]
    #[DataProvider('rows')]
    public function computesRows(int $a, int $b): void
    {
        self::assertSame($a, $b);
    }

    /**
     * @dataProvider rows
     */
    public function testLegacyRows(int $a, int $b): void
    {
        self::assertSame($a, $b);
    }

    public static function rows(): iterable
    {
        yield [1, 1];
    }
}
