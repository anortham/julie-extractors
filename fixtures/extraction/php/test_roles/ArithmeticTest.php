<?php

namespace App\Tests\Billing;

use PHPUnit\Framework\Attributes\After;
use PHPUnit\Framework\Attributes\Before;
use PHPUnit\Framework\Attributes\DataProvider;
use PHPUnit\Framework\Attributes\Test;
use PHPUnit\Framework\TestCase;

final class ArithmeticTest extends TestCase
{
    public static function setUpBeforeClass(): void
    {
    }

    public static function tearDownAfterClass(): void
    {
    }

    protected function setUp(): void
    {
    }

    protected function tearDown(): void
    {
    }

    #[Before]
    public function seedLedger(): void
    {
    }

    #[After]
    public function clearLedger(): void
    {
    }

    /**
     * @before
     */
    public function resetCounters(): void
    {
    }

    /**
     * @after
     */
    public function releaseCounters(): void
    {
    }

    /**
     * @test
     */
    public function itAddsNumbers(): void
    {
    }

    #[Test]
    public function multipliesNumbers(): void
    {
    }

    public function testDividesNumbers(): void
    {
    }

    #[DataProvider('provideRows')]
    public function testAddsRows(int $left, int $right): void
    {
    }

    public static function provideRows(): array
    {
        return [[1, 2], [3, 4]];
    }

    public function buildCalculator(): int
    {
        return 2;
    }
}
