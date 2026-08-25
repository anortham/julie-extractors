<?php

namespace App\Billing;

use PHPUnit\Framework\Attributes\Test;

interface OrderProbe
{
}

final class LegacyOrderTest extends \PHPUnit\Framework\TestCase
{
    protected function setUp(): void
    {
    }

    public function testCalculatesTotal(): void
    {
    }
}

final class AttributeOnlySuite
{
    #[Test]
    public function refundsAnOrder(): void
    {
    }
}

final class ConnectionProbe implements OrderProbe
{
    public function setUp(): void
    {
    }

    public function testConnection(): bool
    {
        return true;
    }
}
