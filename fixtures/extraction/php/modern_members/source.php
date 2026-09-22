<?php

namespace App\Entity;

use Doctrine\ORM\Mapping as ORM;

trait Sluggable
{
    public function slug(): string
    {
        return 'slug';
    }
}

enum Status: string
{
    case Active = 'active';
}

#[ORM\Entity]
#[ORM\Table(name: 'products')]
class Product
{
    use Sluggable;
    use \Illuminate\Database\Eloquent\Factories\HasFactory, Notifiable {
        Notifiable::notify as protected baseNotify;
    }

    public const ROLE = 'product';

    #[ORM\Id]
    #[ORM\GeneratedValue]
    #[ORM\Column]
    private ?int $id = null;

    public function __construct(
        private readonly ProductRepository $products,
        public string $name,
        int $plain,
    ) {
        $this->tags[] = $name;
        [$first, $second] = [$plain, $plain];
        $label = $first . $second;
    }

    public function describe(?Product $other): ?string
    {
        $other?->slug();
        $this->products?->refresh();
        $active = $this->status === Status::Active && self::ROLE === 'product';
        $factory = $this->belongsTo(Category::class);
        return $other?->name;
    }

    public function make(): Product
    {
        $copy = new Product($this->products, $this->name, 1);
        return $copy;
    }
}
