<?php

use Symfony\Component\Routing\Attribute\Route;

#[Route('/api')]
class UserController
{
    #[Route('/users/{id}', methods: ['GET'])]
    public function show($id)
    {
        return $id;
    }

    #[Route('/users', methods: ['POST'])]
    public function create()
    {
        return null;
    }

    // Any-method route: verb omitted.
    #[Route('/webhook')]
    public function webhook()
    {
        return null;
    }

    // Multi-verb array → one fact per method.
    #[Route('/search', methods: ['GET', 'POST'])]
    public function search()
    {
        return null;
    }
}

class DynController
{
    // Silent (M2): interpolated / concatenated paths emit nothing.
    #[Route("/users/$id")]
    public function interpolated($id)
    {
        return $id;
    }

    #[Route('/users/' . $suffix)]
    public function concatenated($suffix)
    {
        return $suffix;
    }
}
