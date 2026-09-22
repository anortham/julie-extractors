use crate::base::{IdentifierKind, RelationshipKind, Symbol, SymbolKind};
use crate::{ExtractionResults, extract_canonical};
use std::path::Path;

fn extract(path: &str, source: &str) -> ExtractionResults {
    extract_canonical(path, source, Path::new("/tmp/test")).expect("php extraction")
}

fn named<'a>(result: &'a ExtractionResults, name: &str) -> Vec<&'a Symbol> {
    result.symbols.iter().filter(|s| s.name == name).collect()
}

fn one<'a>(result: &'a ExtractionResults, name: &str, kind: SymbolKind) -> &'a Symbol {
    let found: Vec<_> = named(result, name)
        .into_iter()
        .filter(|s| s.kind == kind)
        .collect();
    assert_eq!(
        found.len(),
        1,
        "expected one {kind:?} `{name}`, got {found:#?}"
    );
    found[0]
}

fn keys(symbol: &Symbol) -> Vec<&str> {
    symbol
        .annotations
        .iter()
        .map(|a| a.annotation_key.as_str())
        .collect()
}

fn role(symbol: &Symbol) -> Option<&str> {
    symbol.metadata.as_ref()?.get("test_role")?.as_str()
}

#[test]
fn stacked_attribute_groups_give_one_annotation_per_attribute() {
    let source = r#"<?php
final class PriceTest extends TestCase {
    #[Group('slow')]
    #[Test]
    public function computes(): void {}
    #[Test]
    #[DataProvider('rows')]
    public function computesRows(int $a): void {}
    #[Test, DataProvider('rows')]
    public function computesInline(int $a): void {}
    /**
     * @dataProvider rows
     */
    public function testLegacyRows(int $a): void {}
}
"#;
    let result = extract("tests/PriceTest.php", source);
    let computes = one(&result, "computes", SymbolKind::Method);
    assert_eq!(keys(computes), vec!["group", "test"]);
    assert_eq!(role(computes), Some("test_case"));
    let rows = one(&result, "computesRows", SymbolKind::Method);
    assert_eq!(keys(rows), vec!["test", "dataprovider"]);
    assert_eq!(role(rows), Some("parameterized_test"));
    assert_eq!(
        role(one(&result, "testLegacyRows", SymbolKind::Method)),
        Some("parameterized_test")
    );
}

#[test]
fn promoted_constructor_parameters_keep_property_and_parameter() {
    let source = r#"<?php
class OrderService {
    public function __construct(private readonly OrderRepository $orders, int $plain) {}
}
"#;
    let result = extract("src/OrderService.php", source);
    let property = one(&result, "orders", SymbolKind::Property);
    let parameter = one(&result, "orders", SymbolKind::Variable);
    assert_ne!(property.id, parameter.id);
    assert_eq!(
        property.parent_id.as_deref(),
        Some(one(&result, "OrderService", SymbolKind::Class).id.as_str())
    );
    assert_eq!(result.types[&property.id].resolved_type, "OrderRepository");
}

#[test]
fn nullsafe_calls_and_accesses_emit_identifiers_and_calls() {
    let source = r#"<?php
class Invoice {
    public function total(): int { return 1; }
    public function describe(?Invoice $other): ?string {
        $sum = $other?->total();
        $this?->total();
        $pdo?->query('SELECT 2');
        return $other?->customer?->name;
    }
}
"#;
    let result = extract("src/Invoice.php", source);
    let calls: Vec<_> = result
        .identifiers
        .iter()
        .filter(|i| i.kind == IdentifierKind::Call)
        .map(|i| i.name.as_str())
        .collect();
    assert_eq!(calls, vec!["total", "total", "query"]);
    let mut members: Vec<_> = result
        .identifiers
        .iter()
        .filter(|i| i.kind == IdentifierKind::MemberAccess)
        .map(|i| i.name.as_str())
        .collect();
    members.sort();
    assert_eq!(members, vec!["customer", "name"]);
    assert!(
        result
            .relationships
            .iter()
            .any(|r| r.kind == RelationshipKind::Calls)
    );
    assert!(
        result
            .structured_pending_relationships
            .iter()
            .any(|p| p.target.terminal_name == "total"
                && p.target.receiver.as_deref() == Some("other"))
    );
    assert!(result.literals.iter().any(|l| l.literal_text == "SELECT 2"));
}

#[test]
fn assignment_targets_other_than_locals_define_no_variables() {
    let source = r#"<?php
class Holder {
    public function __construct(string $owner, string $tag, array $pair) {
        $this->owner = $owner;
        $this->tags[] = $tag;
        self::$registry[] = $tag;
        [$first, $second] = $pair;
        list('a' => $third) = $pair;
        $local = 1;
    }
}
"#;
    let result = extract("src/Holder.php", source);
    let variables: Vec<_> = result
        .symbols
        .iter()
        .filter(|s| s.kind == SymbolKind::Variable)
        .filter(|s| s.metadata.as_ref().and_then(|m| m.get("role")).is_none())
        .map(|s| s.name.as_str())
        .collect();
    assert_eq!(variables, vec!["first", "second", "third", "local"]);
}

#[test]
fn declaration_kinds_record_no_type_facts_and_returns_are_declared() {
    let source = r#"<?php
namespace App;
use App\Models\Item;
interface Totals {}
trait Discounts {}
enum Currency: string { case Eur = 'eur'; }
class Cart implements Totals {
    const LIMIT = 10;
    public function add(Item $item): void { $count = $this->count(); }
    public function make(): Cart { return new Cart(); }
}
"#;
    let result = extract("src/Cart.php", source);
    let typed: Vec<_> = result
        .types
        .values()
        .map(|t| {
            let name = result
                .symbols
                .iter()
                .find(|s| s.id == t.symbol_id)
                .unwrap()
                .name
                .clone();
            (name, t.resolved_type.clone(), t.is_inferred)
        })
        .collect();
    let mut typed = typed;
    typed.sort();
    assert_eq!(
        typed,
        vec![
            ("add".to_string(), "void".to_string(), false),
            ("item".to_string(), "Item".to_string(), false),
            ("make".to_string(), "Cart".to_string(), false),
        ]
    );
}

#[test]
fn trait_use_gives_uses_relationships_not_imports() {
    let source = r#"<?php
trait Sluggable { public function slug(): string { return 'x'; } }
class Post {
    use Sluggable;
    use \Illuminate\Database\Eloquent\Factories\HasFactory, Notifiable {
        Notifiable::notify as protected baseNotify;
    }
    public function title(): string { return $this->slug(); }
}
"#;
    let result = extract("src/Post.php", source);
    assert!(result.symbols.iter().all(|s| s.kind != SymbolKind::Import));
    let post = one(&result, "Post", SymbolKind::Class);
    let sluggable = one(&result, "Sluggable", SymbolKind::Trait);
    assert!(
        result
            .relationships
            .iter()
            .any(|r| r.kind == RelationshipKind::Uses
                && r.from_symbol_id == post.id
                && r.to_symbol_id == sluggable.id)
    );
    let pending: Vec<_> = result
        .structured_pending_relationships
        .iter()
        .filter(|p| p.pending.kind == RelationshipKind::Uses)
        .map(|p| {
            (
                p.target.terminal_name.as_str(),
                p.target.namespace_path.len(),
            )
        })
        .collect();
    assert_eq!(pending, vec![("HasFactory", 4), ("Notifiable", 0)]);
    assert!(
        !result
            .identifiers
            .iter()
            .any(|i| matches!(i.name.as_str(), "Notifiable" | "baseNotify" | "notify"))
    );
}

#[test]
fn docblock_test_substrings_are_not_test_tags() {
    let source = r#"<?php
namespace App;
class Mailer {
    /**
     * Send the digest, e.g. to qa@testing.example.com.
     */
    public function sendDigest(string $to): void {}
    /**
     * @tested-by MailerTest::testSend
     * @testdox sends mail
     */
    public function send(): void {}
    /** @test */
    public function realCase(): void {}
}
"#;
    let result = extract("src/Mailer.php", source);
    assert_eq!(role(one(&result, "sendDigest", SymbolKind::Method)), None);
    assert_eq!(role(one(&result, "send", SymbolKind::Method)), None);
    assert_eq!(
        role(one(&result, "realCase", SymbolKind::Method)),
        Some("test_case")
    );
}

#[test]
fn class_constant_access_gives_type_usage_and_member_access() {
    let source = r#"<?php
enum Status: string { case Active = 'active'; }
class User {
    public const ROLE = 'user';
    public function team() { return $this->belongsTo(Team::class); }
    public function isActive(): bool {
        return $this->status === Status::Active && self::ROLE === 'user' && static::$registry;
    }
}
"#;
    let result = extract("src/User.php", source);
    let rows: Vec<_> = result
        .identifiers
        .iter()
        .filter(|i| {
            matches!(
                i.name.as_str(),
                "Team" | "Status" | "Active" | "ROLE" | "registry"
            )
        })
        .map(|i| (i.name.as_str(), i.kind.clone(), i.receiver_type.as_deref()))
        .collect();
    assert_eq!(
        rows,
        vec![
            ("Team", IdentifierKind::TypeUsage, None),
            ("Active", IdentifierKind::MemberAccess, None),
            ("Status", IdentifierKind::VariableRef, None),
            ("ROLE", IdentifierKind::MemberAccess, Some("User")),
            ("registry", IdentifierKind::MemberAccess, Some("User")),
        ]
    );
}
