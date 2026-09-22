use crate::base::{IdentifierKind, RelationshipKind, StructuralFact, Symbol, SymbolKind};
use crate::tests::helpers::{facts_with_pattern, metadata_str};
use crate::{ExtractionResults, extract_canonical};
use std::path::Path;

fn extract(path: &str, source: &str) -> ExtractionResults {
    extract_canonical(path, source, Path::new("/tmp/test")).expect("php extraction")
}

fn one<'a>(result: &'a ExtractionResults, name: &str) -> &'a Symbol {
    let found: Vec<_> = result.symbols.iter().filter(|s| s.name == name).collect();
    assert_eq!(found.len(), 1, "expected one `{name}`, got {found:#?}");
    found[0]
}

fn meta<'a>(symbol: &'a Symbol, key: &str) -> Option<&'a str> {
    symbol.metadata.as_ref()?.get(key)?.as_str()
}

fn role(symbol: &Symbol) -> Option<&str> {
    meta(symbol, "test_role")
}

fn symbol_name(result: &ExtractionResults, id: &str) -> String {
    result
        .symbols
        .iter()
        .find(|s| s.id == id)
        .map(|s| s.name.clone())
        .unwrap_or_default()
}

fn edges(result: &ExtractionResults, kind: RelationshipKind) -> Vec<(String, String)> {
    let mut edges: Vec<_> = result
        .relationships
        .iter()
        .filter(|r| r.kind == kind)
        .map(|r| {
            (
                symbol_name(result, &r.from_symbol_id),
                symbol_name(result, &r.to_symbol_id),
            )
        })
        .collect();
    edges.sort();
    edges
}

fn pending(result: &ExtractionResults) -> Vec<(String, String, Option<String>, u32)> {
    let mut rows: Vec<_> = result
        .structured_pending_relationships
        .iter()
        .map(|p| {
            (
                format!("{:?}", p.pending.kind),
                p.target.display_name.clone(),
                p.target.receiver.clone(),
                p.pending.line_number,
            )
        })
        .collect();
    rows.sort();
    rows
}

fn fact_values<'a>(facts: &[&'a StructuralFact], key: &str) -> Vec<&'a str> {
    facts
        .iter()
        .filter_map(|fact| metadata_str(fact, key))
        .collect()
}

#[test]
fn chained_calls_with_expression_receivers_have_no_receiver_and_their_own_line() {
    let source = r#"<?php
class UserRepo {
    public function active(Builder $query): mixed {
        $x = Order::where('paid', true)
            ->latest()
            ->get();
        return $query->whereNotNull('email')
            ->first();
    }
    public function run(callable $cb, string $name) {
        $cb(1);
        $obj->$name();
        (new Worker())->handle();
    }
}
"#;
    let result = extract("UserRepo.php", source);
    let rows = pending(&result);
    for expected in [
        ("Calls", "Order.where", Some("Order"), 4),
        ("Calls", "latest", None, 5),
        ("Calls", "get", None, 6),
        ("Calls", "query.whereNotNull", Some("query"), 7),
        ("Calls", "first", None, 8),
        ("Calls", "handle", None, 13),
        ("Instantiates", "Worker", None, 13),
    ] {
        let expected = (
            expected.0.to_string(),
            expected.1.to_string(),
            expected.2.map(str::to_string),
            expected.3,
        );
        assert!(rows.contains(&expected), "{expected:?} not in {rows:#?}");
    }
    assert!(
        !rows
            .iter()
            .any(|(_, display, _, _)| display.contains('$') || display.contains('(')),
        "{rows:#?}"
    );
    assert!(
        !result
            .identifiers
            .iter()
            .any(|i| i.kind == IdentifierKind::Call && i.name.starts_with('$'))
    );
}

#[test]
fn static_self_and_own_class_calls_resolve_to_the_enclosing_class() {
    let source = r#"<?php
class Money {
    public static function of(int $v): static {
        self::validate($v);
        static::validate($v);
        Money::validate($v);
        new static();
        return new self();
    }
    private static function validate(int $v): void {}
}
"#;
    let result = extract("Money.php", source);
    assert_eq!(
        edges(&result, RelationshipKind::Calls),
        vec![("of".to_string(), "validate".to_string()); 3]
    );
    assert_eq!(
        edges(&result, RelationshipKind::Instantiates),
        vec![("of".to_string(), "Money".to_string()); 2]
    );
    assert!(result.structured_pending_relationships.is_empty());
    let of = one(&result, "of");
    let fact = result.types.get(&of.id).expect("return type fact");
    assert_eq!(fact.resolved_type, "Money");
}

#[test]
fn enums_and_anonymous_classes_extend_and_implement_and_traits_are_used() {
    let source = r#"<?php
interface HasLabel { public function label(): string; }
trait Loggable { public function log(): void {} }
class Job {}
enum Suit: string implements HasLabel, \JsonSerializable { case Hearts = 'H'; public function label(): string { return 'h'; } }
class SendMail extends Job { use Loggable; }
function make(): object {
    return new class extends Job implements HasLabel { public function label(): string { return 'x'; } };
}
"#;
    let result = extract("types.php", source);
    assert_eq!(
        edges(&result, RelationshipKind::Implements),
        vec![
            ("Suit".to_string(), "HasLabel".to_string()),
            ("anonymous_class_L8".to_string(), "HasLabel".to_string()),
        ]
    );
    assert_eq!(
        edges(&result, RelationshipKind::Extends),
        vec![
            ("SendMail".to_string(), "Job".to_string()),
            ("anonymous_class_L8".to_string(), "Job".to_string()),
        ]
    );
    assert_eq!(
        edges(&result, RelationshipKind::Uses),
        vec![("SendMail".to_string(), "Loggable".to_string())]
    );
    let rows = pending(&result);
    assert_eq!(
        rows,
        vec![(
            "Implements".to_string(),
            "JsonSerializable".to_string(),
            None,
            5
        )]
    );
}

#[test]
fn string_type_keywords_are_not_string_literal_regions() {
    let source = "<?php\nfunction greet(string $name): string\n{\n    return \"hi {$name}\";\n}\n";
    let result = extract("greet.php", source);
    let lines: Vec<_> = result
        .source_regions
        .iter()
        .filter(|r| format!("{:?}", r.kind) == "StringLiteral")
        .map(|r| r.start_line)
        .collect();
    assert_eq!(lines, vec![4]);
}

#[test]
fn every_use_clause_is_an_import_with_its_kind_and_one_fact() {
    let source = r#"<?php
namespace App\Support;
use Closure;
use Carbon\Carbon, Illuminate\Support\Str;
use function sprintf;
use const PHP_ROUND_HALF_UP;
use App\Models\{User, Post as BlogPost};
use Throwable as BaseThrowable;
final class Helper { use \App\Concerns\HasUuid, Notifiable; }
"#;
    let result = extract("Helper.php", source);
    let mut imports: Vec<_> = result
        .symbols
        .iter()
        .filter(|s| s.kind == SymbolKind::Import)
        .map(|s| {
            (
                s.name.as_str(),
                meta(s, "alias"),
                meta(s, "importKind"),
                s.body_span.is_some(),
            )
        })
        .collect();
    imports.sort();
    assert_eq!(
        imports,
        vec![
            ("App\\Models\\Post", Some("BlogPost"), None, false),
            ("App\\Models\\User", None, None, false),
            ("Carbon\\Carbon", None, None, false),
            ("Closure", None, None, false),
            ("Illuminate\\Support\\Str", None, None, false),
            ("PHP_ROUND_HALF_UP", None, Some("const"), false),
            ("Throwable", Some("BaseThrowable"), None, false),
            ("sprintf", None, Some("function"), false),
        ]
    );
    let import_facts = facts_with_pattern(&result, "php.namespace_use_declaration.v1");
    assert_eq!(
        fact_values(&import_facts, "import_target"),
        vec![
            "Closure",
            "Carbon\\Carbon",
            "Illuminate\\Support\\Str",
            "sprintf",
            "PHP_ROUND_HALF_UP",
            "App\\Models\\User",
            "App\\Models\\Post",
            "Throwable",
        ]
    );
    assert_eq!(
        fact_values(&import_facts, "import_kind"),
        vec!["function", "const"]
    );
    let traits = facts_with_pattern(&result, "php.trait_use_declaration.v1");
    assert_eq!(
        fact_values(&traits, "trait_name"),
        vec!["\\App\\Concerns\\HasUuid", "Notifiable"]
    );
}

#[test]
fn new_and_qualified_names_give_terminal_identifiers_with_a_qualifier() {
    let source = r#"<?php
class Checkout {
    public function run(\Illuminate\Http\Request $request): \Illuminate\Http\JsonResponse {
        $order = new Order($request->all());
        $mail = new \App\Mail\Receipt($order);
        \App\Support\audit($order);
        try {} catch (\RuntimeException $e) {}
    }
}
"#;
    let result = extract("Checkout.php", source);
    let mut idents: Vec<_> = result
        .identifiers
        .iter()
        .filter(|i| matches!(i.kind, IdentifierKind::TypeUsage | IdentifierKind::Call))
        .map(|i| {
            (
                i.name.as_str(),
                i.start_line,
                i.metadata
                    .as_ref()
                    .and_then(|m| m.get("namespace_qualifier"))
                    .and_then(|v| v.as_str()),
            )
        })
        .filter(|(name, _, _)| *name != "all")
        .collect();
    idents.sort();
    assert_eq!(
        idents,
        vec![
            ("JsonResponse", 3, Some("\\Illuminate\\Http")),
            ("Order", 4, None),
            ("Receipt", 5, Some("\\App\\Mail")),
            ("Request", 3, Some("\\Illuminate\\Http")),
            ("RuntimeException", 7, None),
            ("audit", 6, Some("\\App\\Support")),
        ]
    );
}

#[test]
fn each_const_and_property_element_is_its_own_symbol() {
    let source = r#"<?php
const MIN_AGE = 18, MAX_AGE = 99;
class Limits {
    public const LOW = 1, HIGH = 9;
    protected int $min = 0, $max = 100;
    public $x, $y;
}
"#;
    let result = extract("Limits.php", source);
    for name in ["MIN_AGE", "MAX_AGE", "LOW", "HIGH"] {
        assert_eq!(one(&result, name).kind, SymbolKind::Constant, "{name}");
    }
    for name in ["min", "max", "x", "y"] {
        assert_eq!(one(&result, name).kind, SymbolKind::Property, "{name}");
    }
    assert_eq!(meta(one(&result, "max"), "propertyType"), Some("int"));
    assert_eq!(
        one(&result, "HIGH").signature.as_deref(),
        Some("public const HIGH = 9")
    );
}

#[test]
fn heredoc_nowdoc_and_framework_sql_are_captured_and_request_query_is_not() {
    let source = r#"<?php
class Report {
    public function run(Request $request) {
        $this->pdo->exec(<<<SQL
            UPDATE users
              SET active = 1
            SQL);
        $this->pdo->prepare(<<<'SQL'
            DELETE FROM sessions WHERE id = :id
            SQL);
        \DB::statement('DROP TABLE tmp');
        User::whereRaw('created_at > now()')->get();
        $page = $request->query('per_page', 15);
    }
}
"#;
    let result = extract("Report.php", source);
    let literals: Vec<_> = result
        .literals
        .iter()
        .map(|l| {
            (
                l.literal_text.as_str(),
                l.carrier.as_deref().unwrap_or_default(),
            )
        })
        .collect();
    for expected in [
        ("UPDATE users\n  SET active = 1", "$this->pdo.exec"),
        ("DELETE FROM sessions WHERE id = :id", "$this->pdo.prepare"),
        ("DROP TABLE tmp", "DB.statement"),
        ("created_at > now()", "User.whereRaw"),
    ] {
        assert!(
            literals.contains(&expected),
            "{expected:?} not in {literals:?}"
        );
    }
    let policy = crate::language_policy::literal_carrier_policy("php").expect("php policy");
    let mut classified = result.literals.clone();
    crate::language_policy::classify_literals_with_policies(
        &mut classified,
        &std::collections::HashMap::from([("php".to_string(), policy.clone())]),
    );
    let sql: Vec<_> = classified.iter().map(|l| l.literal_text.as_str()).collect();
    assert_eq!(sql.len(), 4, "{sql:?}");
    assert!(!sql.contains(&"per_page"));
}

#[test]
fn union_intersection_dnf_and_never_types_stay_in_signatures() {
    let source = r#"<?php
class Types {
    public int|string $id;
    public Countable&Traversable $items;
    public (A&B)|null $dnf;
    public const string PREFIX = 'x';
    public function fail(): never { throw new Exception(); }
    public function both(): Countable&Traversable { return $this->items; }
    public function pick(): (A&B)|null { return null; }
}
"#;
    let result = extract("Types.php", source);
    for (name, signature) in [
        ("id", "public int|string $id"),
        ("items", "public Countable&Traversable $items"),
        ("dnf", "public (A&B)|null $dnf"),
        ("PREFIX", "public const string PREFIX = 'x'"),
        ("fail", "public function fail(): never"),
        ("both", "public function both(): Countable&Traversable"),
        ("pick", "public function pick(): (A&B)|null"),
    ] {
        assert_eq!(one(&result, name).signature.as_deref(), Some(signature));
    }
    assert_eq!(
        meta(one(&result, "items"), "propertyType"),
        Some("Countable&Traversable")
    );
    assert_eq!(meta(one(&result, "fail"), "returnType"), Some("never"));
}

#[test]
fn function_definitions_are_functions_even_inside_namespaces_and_closures() {
    let source = r#"<?php
namespace App\Support {
    function slugify(string $s): string { return strtolower($s); }
}
namespace {
    function boot(): void {
        $register = function () { function late_helper(): int { return 1; } };
    }
}
"#;
    let result = extract("helpers.php", source);
    assert_eq!(one(&result, "slugify").kind, SymbolKind::Function);
    assert_eq!(one(&result, "late_helper").kind, SymbolKind::Function);
}

#[test]
fn include_require_facts_and_define_constants() {
    let source = r#"<?php
require_once __DIR__ . '/vendor/autoload.php';
require 'config/app.php';
include_once 'lib/helpers.php';
include $dynamic;
define('APP_ENV', 'production');
function env_name(): string { return APP_ENV; }
"#;
    let result = extract("bootstrap.php", source);
    let includes = facts_with_pattern(&result, "php.include_call.v1");
    assert_eq!(
        fact_values(&includes, "include_kind"),
        vec!["require_once", "require", "include_once", "include"]
    );
    assert_eq!(
        fact_values(&includes, "included_path"),
        vec!["/vendor/autoload.php", "config/app.php", "lib/helpers.php"]
    );
    assert_eq!(fact_values(&includes, "path_base"), vec!["__DIR__"]);
    let app_env = one(&result, "APP_ENV");
    assert_eq!(app_env.kind, SymbolKind::Constant);
    assert_eq!(meta(app_env, "value"), Some("'production'"));
}

#[test]
fn pest_symbols_do_not_call_the_dsl_that_declares_them() {
    let source = r#"<?php
beforeEach(function () { $this->user = makeUser(); });
it('creates users', function (string $email) { $user = createUser($email); })->with(['a@b.c']);
describe('admin', function () { it('can ban', fn () => expect(true)->toBeTrue()); });
"#;
    let result = extract("tests/Feature/UserTest.php", source);
    assert!(edges(&result, RelationshipKind::Calls).is_empty());
    let targets: Vec<_> = pending(&result)
        .into_iter()
        .map(|(_, display, _, _)| display)
        .collect();
    for dsl in ["it", "describe", "beforeEach", "with"] {
        assert!(!targets.contains(&dsl.to_string()), "{dsl}: {targets:?}");
    }
    for inner in ["makeUser", "createUser", "expect", "toBeTrue"] {
        assert!(targets.contains(&inner.to_string()), "{inner}: {targets:?}");
    }
    assert!(one(&result, "creates users").body_span.is_some());
}

#[test]
fn laravel_routes_join_controller_groups_invokables_and_names() {
    let source = r#"<?php
Route::get('/dashboard', ShowDashboard::class)->name('dashboard');
Route::controller(ProfileController::class)->name('profile.')->group(function () {
    Route::get('/profile', 'show')->name('show');
    Route::patch('/profile', 'update');
});
"#;
    let result = extract("routes/web.php", source);
    let routes = facts_with_pattern(&result, "laravel.route.v1");
    assert_eq!(
        fact_values(&routes, "controller_action"),
        vec![
            "ShowDashboard@__invoke",
            "ProfileController@show",
            "ProfileController@update"
        ]
    );
    assert_eq!(
        fact_values(&routes, "route_name"),
        vec!["dashboard", "profile.show"]
    );
}

#[test]
fn a_group_that_loads_a_route_file_emits_its_prefix_with_the_file_as_target() {
    let source = r#"<?php
class RouteServiceProvider extends ServiceProvider {
    public function boot(): void {
        $this->routes(function () {
            Route::prefix('api')->middleware('api')->group(base_path('routes/api.php'));
            Route::middleware('web')->group(base_path('routes/web.php'));
        });
    }
}
"#;
    let result = extract("app/Providers/RouteServiceProvider.php", source);
    let prefixes = facts_with_pattern(&result, "laravel.route_prefix.v1");
    assert_eq!(prefixes.len(), 1, "{prefixes:#?}");
    assert_eq!(metadata_str(prefixes[0], "mount_path"), Some("api"));
    assert_eq!(
        metadata_str(prefixes[0], "mount_target"),
        Some("routes/api.php")
    );
}

#[test]
fn symfony_route_names_join_the_class_name_prefix() {
    let source = r#"<?php
use Symfony\Component\Routing\Attribute\Route;
#[Route('/api/products', name: 'api_products_')]
class ProductController {
    #[Route('', name: 'list', methods: ['GET'])]
    public function list(): array { return []; }
    #[Route('/{id}', methods: ['GET'])]
    public function show(int $id): array { return []; }
}
"#;
    let result = extract("src/Controller/ProductController.php", source);
    let routes = facts_with_pattern(&result, "symfony.route.v1");
    assert_eq!(
        fact_values(&routes, "route_name"),
        vec!["api_products_list"]
    );
}

#[test]
fn typed_class_properties_prove_http_client_receivers() {
    let source = r#"<?php
use GuzzleHttp\Client;
use Symfony\Contracts\HttpClient\HttpClientInterface;
class GithubClient {
    private Client $http;
    private $untyped;
    public function __construct(private readonly HttpClientInterface $symfony) { $this->http = new Client(); }
    public function repos(): array {
        $a = $this->http->get('https://api.github.com/user/repos');
        $b = $this->symfony->request('GET', 'https://api.github.com/user/orgs');
        $c = $this->untyped->get('https://api.github.com/none');
        return [$a, $b, $c];
    }
}
"#;
    let result = extract("GithubClient.php", source);
    let requests = facts_with_pattern(&result, "http.client_request.v1");
    assert_eq!(
        fact_values(&requests, "target_path"),
        vec![
            "https://api.github.com/user/repos",
            "https://api.github.com/user/orgs"
        ]
    );
    assert_eq!(
        fact_values(&requests, "client"),
        vec!["guzzle", "symfony_http_client"]
    );
}

#[test]
fn codeception_cests_and_phpspec_specs_are_test_suites() {
    let cest = r#"<?php
class LoginCest {
    public function _before(AcceptanceTester $I): void {}
    public function loginWorks(AcceptanceTester $I): void {}
    public function _after(AcceptanceTester $I): void {}
    protected function helper(AcceptanceTester $I): void {}
    public function describe(): string { return 'x'; }
}
"#;
    let result = extract("tests/acceptance/LoginCest.php", cest);
    assert_eq!(role(one(&result, "LoginCest")), Some("test_container"));
    assert_eq!(role(one(&result, "_before")), Some("fixture_setup"));
    assert_eq!(role(one(&result, "loginWorks")), Some("test_case"));
    assert_eq!(role(one(&result, "_after")), Some("fixture_teardown"));
    assert_eq!(role(one(&result, "helper")), None);
    assert_eq!(role(one(&result, "describe")), None);

    let spec = r#"<?php
use PhpSpec\ObjectBehavior;
class MoneySpec extends ObjectBehavior {
    function let() {}
    function letGo() {}
    function it_is_initializable() {}
    function its_amount_is_positive() {}
    function helper() {}
}
"#;
    let result = extract("spec/MoneySpec.php", spec);
    assert_eq!(role(one(&result, "MoneySpec")), Some("test_container"));
    assert_eq!(role(one(&result, "let")), Some("fixture_setup"));
    assert_eq!(role(one(&result, "letGo")), Some("fixture_teardown"));
    assert_eq!(role(one(&result, "it_is_initializable")), Some("test_case"));
    assert_eq!(
        role(one(&result, "its_amount_is_positive")),
        Some("test_case")
    );
    assert_eq!(role(one(&result, "helper")), None);
}

#[test]
fn bodiless_methods_imports_properties_and_plain_variables_have_no_body() {
    let source = r#"<?php
use App\Models\{User, Post as BlogPost};
interface Repository { public function find(int $id): ?array; }
abstract class Base {
    #[ORM\Column(type: 'string')]
    protected (Countable&Traversable)|null $items;
    abstract protected function handle(array $payload): void;
    public function total(array $ratios): int {
        $total = array_sum($ratios);
        $format = function ($v) { return (string) $v; };
        return $total;
    }
}
"#;
    let result = extract("Base.php", source);
    let body_of = |name: &str, kind: SymbolKind| {
        result
            .symbols
            .iter()
            .find(|s| s.name == name && s.kind == kind)
            .unwrap_or_else(|| panic!("no {kind:?} {name}"))
            .body_span
            .is_some()
    };
    assert!(!body_of("find", SymbolKind::Method));
    assert!(!body_of("handle", SymbolKind::Method));
    assert!(!body_of("items", SymbolKind::Property));
    assert!(!body_of("App\\Models\\User", SymbolKind::Import));
    assert!(!body_of("total", SymbolKind::Variable));
    assert!(body_of("total", SymbolKind::Method));
    assert!(body_of("format", SymbolKind::Variable));
}
