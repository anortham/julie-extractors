<?php

namespace App\Settings;

use App\Support\Context;

class Settings implements Context
{
    #[Given('defaults')]
    public function defaults(): void
    {
    }

    public function testConnection(): bool
    {
        return true;
    }
}
