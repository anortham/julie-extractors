<?php
// Deferred PHP HTTP clients: Symfony HttpClient, cURL, and Guzzle request().
// Backs closure of php.http_client.deferred.

use Symfony\Contracts\HttpClient\HttpClientInterface;
use Symfony\Component\HttpClient\HttpClient;
use GuzzleHttp\Client;

function deferredClients(HttpClientInterface $symfony, Client $guzzle)
{
    $symfony->request('PATCH', 'https://api.example.com/items/1');
    HttpClient::create()->request('GET', 'https://api.example.com/status');

    $guzzle->request('GET', '/users');
    $guzzle->requestAsync('POST', '/items');

    $direct = curl_init('https://api.example.com/health');
    $ch = curl_init();
    curl_setopt($ch, CURLOPT_URL, '/curl/items');
    curl_setopt($ch, CURLOPT_CUSTOMREQUEST, 'DELETE');

    // Silent: dynamic method/url, curl without URL.
    $guzzle->request($method, '/users');
    $guzzle->request('GET', $url);
    $noUrl = curl_init();
    curl_setopt($noUrl, CURLOPT_CUSTOMREQUEST, 'DELETE');
}
