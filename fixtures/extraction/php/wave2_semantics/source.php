<?php
namespace App\Billing;

require_once __DIR__ . '/../vendor/autoload.php';
include_once 'lib/helpers.php';

use Closure;
use Carbon\Carbon, Illuminate\Support\Str;
use function sprintf;
use const PHP_ROUND_HALF_UP;
use App\Models\{User, Post as BlogPost};
use GuzzleHttp\Client;
use Symfony\Contracts\HttpClient\HttpClientInterface;

define('BILLING_VERSION', '2.1.0');
const MIN_CENTS = 1, MAX_CENTS = 1000000;

interface HasLabel { public function label(): string; }
interface Repository { public function find(int $id): ?array; }
trait Loggable { public function log(string $message): void {} }

enum Currency: string implements HasLabel, \JsonSerializable {
    case Usd = 'usd';
    public function label(): string { return strtoupper($this->value); }
    public function jsonSerialize(): mixed { return $this->value; }
}

abstract class Job { abstract protected function handle(array $payload): void; }

final class Money extends Job {
    use Loggable;

    public const LOW = 1, HIGH = 9;
    public const string PREFIX = 'm_';
    protected int $cents = 0, $scale = 2;
    private int|string $key;
    private Countable&Traversable $ledger;
    private Client $http;

    public function __construct(private readonly HttpClientInterface $symfony)
    {
        $this->http = new Client();
    }

    public static function of(int $cents): static
    {
        self::validate($cents);
        static::validate($cents);
        Money::validate($cents);
        return new static();
    }

    private static function validate(int $cents): void {}

    protected function handle(array $payload): void
    {
        $rows = Order::where('paid', true)
            ->latest()
            ->get();
        $total = array_sum($payload);
        $format = function ($value) { return sprintf('%d', $value); };
        (new Worker())->handle();
        \App\Support\audit($rows);
    }

    public function fail(): never { throw new \RuntimeException('no'); }

    public function refresh(\PDO $pdo): void
    {
        $pdo->exec(<<<SQL
            UPDATE balances
              SET stale = 0
            SQL);
        \DB::statement('VACUUM balances');
    }

    public function remote(): array
    {
        $a = $this->http->get('https://api.example.com/rates');
        $b = $this->symfony->request('GET', 'https://api.example.com/fees');
        return [$a, $b];
    }

    public function label(): object
    {
        return new class extends Job implements HasLabel {
            protected function handle(array $payload): void {}
            public function label(): string { return 'anon'; }
        };
    }
}
