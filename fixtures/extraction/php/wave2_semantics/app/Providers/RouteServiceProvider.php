<?php
namespace App\Providers;

use Illuminate\Support\Facades\Route;

class RouteServiceProvider extends ServiceProvider
{
    public function boot(): void
    {
        $this->routes(function () {
            Route::prefix('api')->middleware('api')->group(base_path('routes/api.php'));
        });
    }
}
