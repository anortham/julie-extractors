<?php
use Illuminate\Support\Facades\Route;

Route::get('/dashboard', ShowDashboard::class)->name('dashboard');
Route::controller(InvoiceController::class)->name('invoices.')->group(function () {
    Route::get('/invoices/{id}', 'show')->name('show');
    Route::post('/invoices', 'store');
});
