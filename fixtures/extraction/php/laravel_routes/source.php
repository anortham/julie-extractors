<?php

use Illuminate\Support\Facades\Route;
use App\Http\Controllers\UserController;
use App\Http\Controllers\PhotoController;
use App\Http\Controllers\BookController;
use App\Http\Controllers\AdminController;

// Top-level verb routes with array controller actions.
Route::get('/users/{id}', [UserController::class, 'show']);
Route::post('/users', [UserController::class, 'store']);
Route::delete('/users/{id}', [UserController::class, 'destroy']);

// Route::any is not verb-restricted.
Route::any('/webhook', [UserController::class, 'webhook']);

// Resource routes (7 RESTful actions) and apiResource (5, no create/edit).
Route::resource('photos', PhotoController::class);
Route::apiResource('books', BookController::class);

// Same-file prefix group, member-call chain form.
Route::prefix('admin')->group(function () {
    Route::get('/dashboard', [AdminController::class, 'dashboard']);
    Route::get('/users/{id}', [AdminController::class, 'showUser']);
});

// Same-file prefix group, array-config form.
Route::group(['prefix' => 'api', 'middleware' => 'auth'], function () {
    Route::get('/status', [UserController::class, 'status']);
});

// Silent (M2 doctrine): interpolated / concatenated / const route args emit
// nothing — a false "static" would promote a computed path to a guessed route.
Route::get("/users/$id", [UserController::class, 'show']);
Route::get('/users/' . $suffix, [UserController::class, 'show']);
Route::get(self::LEGACY_PATH, [UserController::class, 'legacy']);
