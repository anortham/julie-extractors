<?php

use GuzzleHttp\Client;
use Illuminate\Support\Facades\Http;

function fetchUsers(Client $client, $id, $base)
{
    // Guzzle: receiver.verb('url') static-literal requests.
    $client->get('https://api.example.com/users');
    $client->post('/items');

    // Laravel Http facade: direct and chained.
    Http::get('https://api.example.com/status');
    Http::withToken('token')->post('/webhooks');

    // Silent (M2 doctrine): interpolated / concatenated / variable URLs emit
    // nothing.
    $client->get('/users/' . $id);
    $client->get($endpoint);
    Http::get("$base/health");
}
