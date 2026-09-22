<?php
namespace App\Controller;

use Symfony\Component\Routing\Attribute\Route;

#[Route('/api/products', name: 'api_products_')]
class ProductController
{
    #[Route('', name: 'list', methods: ['GET'])]
    public function list(): array { return []; }
}
