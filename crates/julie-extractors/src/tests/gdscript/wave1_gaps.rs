use crate::base::{ExtractionResults, IdentifierKind, RelationshipKind, Symbol, SymbolKind};
use crate::extract_canonical;
use std::path::Path;

fn extract(path: &str, code: &str) -> ExtractionResults {
    extract_canonical(path, code, Path::new("/tmp/test")).expect("gdscript extraction")
}

fn symbol<'a>(result: &'a ExtractionResults, name: &str, kind: SymbolKind) -> &'a Symbol {
    result
        .symbols
        .iter()
        .find(|s| s.name == name && s.kind == kind)
        .unwrap_or_else(|| panic!("no {kind:?} {name}: {:#?}", names(result)))
}

fn names(result: &ExtractionResults) -> Vec<String> {
    result
        .symbols
        .iter()
        .map(|s| format!("{}:{:?}", s.name, s.kind))
        .collect()
}

fn name_of(result: &ExtractionResults, id: Option<&String>) -> String {
    id.and_then(|id| result.symbols.iter().find(|s| &s.id == id))
        .map(|s| s.name.clone())
        .unwrap_or_default()
}

fn base_types(symbol: &Symbol) -> Vec<String> {
    symbol
        .metadata
        .as_ref()
        .and_then(|m| m.get("base_types"))
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

fn test_role(symbol: &Symbol) -> String {
    symbol
        .metadata
        .as_ref()
        .and_then(|m| m.get("test_role"))
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string()
}

fn calls_from(result: &ExtractionResults, caller: &str) -> Vec<String> {
    let mut out: Vec<String> = result
        .relationships
        .iter()
        .filter(|r| r.kind == RelationshipKind::Calls)
        .filter(|r| name_of(result, Some(&r.from_symbol_id)) == caller)
        .map(|r| name_of(result, Some(&r.to_symbol_id)))
        .collect();
    out.extend(
        result
            .structured_pending_relationships
            .iter()
            .filter(|p| p.pending.kind == RelationshipKind::Calls)
            .filter(|p| name_of(result, Some(&p.pending.from_symbol_id)) == caller)
            .map(|p| format!("pending:{}", p.target.terminal_name)),
    );
    out
}

fn pending_receivers(result: &ExtractionResults) -> Vec<(String, String)> {
    result
        .structured_pending_relationships
        .iter()
        .filter(|p| p.pending.kind == RelationshipKind::Calls)
        .map(|p| {
            (
                p.target.terminal_name.clone(),
                p.target.receiver.clone().unwrap_or_default(),
            )
        })
        .collect()
}

#[test]
fn inner_class_spans_its_definition_and_owns_its_members() {
    let code = "class_name Inventory\nextends Resource\nclass ItemStack extends RefCounted:\n\tvar count: int = 0\n\tconst LIMIT := 99\n\tsignal changed\n\tfunc add(n: int) -> void:\n\t\tcount += n\n\tclass Nested:\n\t\tfunc deep() -> void: pass\n\tfunc push(x) -> void: pass\nclass Special extends ItemStack:\n\tfunc add(n: int) -> void:\n\t\tsuper.add(n * 2)\n";
    let result = extract("inventory.gd", code);
    let stack = symbol(&result, "ItemStack", SymbolKind::Class);
    assert_eq!((stack.start_line, stack.end_line), (3, 11));
    assert!(stack.body_span.is_some());
    assert_eq!(base_types(stack), vec!["RefCounted"]);
    assert!(
        stack
            .signature
            .as_deref()
            .unwrap()
            .contains("extends RefCounted")
    );
    let inventory = symbol(&result, "Inventory", SymbolKind::Class);
    assert_eq!(stack.parent_id.as_ref(), Some(&inventory.id));
    for (name, kind) in [
        ("count", SymbolKind::Field),
        ("LIMIT", SymbolKind::Constant),
        ("changed", SymbolKind::Event),
        ("Nested", SymbolKind::Class),
        ("push", SymbolKind::Method),
    ] {
        assert_eq!(
            name_of(&result, symbol(&result, name, kind).parent_id.as_ref()),
            "ItemStack",
            "{name}"
        );
    }
    let deep = symbol(&result, "deep", SymbolKind::Method);
    assert_eq!(name_of(&result, deep.parent_id.as_ref()), "Nested");
    let special = symbol(&result, "Special", SymbolKind::Class);
    assert!(
        result
            .relationships
            .iter()
            .any(|r| r.kind == RelationshipKind::Extends
                && r.from_symbol_id == special.id
                && r.to_symbol_id == stack.id)
    );
}

#[test]
fn class_name_script_is_one_class_that_owns_every_top_level_member() {
    let code = "extends Node\nclass_name Spawner\n\nsignal spawned\nconst MAX = 3\nenum Mood { CALM }\nfunc before_inner() -> void:\n\tpass\n\nclass Wave:\n\tvar size := 3\n\nvar after_field := 1\n\nfunc after_inner() -> void:\n\tbefore_inner()\n";
    let result = extract("spawner.gd", code);
    let classes: Vec<&Symbol> = result
        .symbols
        .iter()
        .filter(|s| s.kind == SymbolKind::Class && s.parent_id.is_none())
        .collect();
    assert_eq!(classes.len(), 1, "{:#?}", names(&result));
    let spawner = classes[0];
    assert_eq!(spawner.name, "Spawner");
    assert_eq!(base_types(spawner), vec!["Node"]);
    for (name, kind) in [
        ("spawned", SymbolKind::Event),
        ("MAX", SymbolKind::Constant),
        ("Mood", SymbolKind::Enum),
        ("before_inner", SymbolKind::Method),
        ("Wave", SymbolKind::Class),
        ("after_field", SymbolKind::Field),
        ("after_inner", SymbolKind::Method),
    ] {
        assert_eq!(
            symbol(&result, name, kind).parent_id.as_ref(),
            Some(&spawner.id),
            "{name}"
        );
    }
    assert_eq!(
        result
            .structured_pending_relationships
            .iter()
            .filter(|p| p.pending.kind == RelationshipKind::Extends)
            .count(),
        0,
        "Node is a builtin base"
    );
    assert_eq!(calls_from(&result, "after_inner"), vec!["before_inner"]);
}

#[test]
fn one_line_class_name_extends_records_the_base_and_one_extends_edge() {
    let result = extract(
        "boss.gd",
        "class_name Boss extends Enemy\nsignal defeated\n\nfunc attack() -> void:\n\tcharge()\n",
    );
    let boss = symbol(&result, "Boss", SymbolKind::Class);
    assert_eq!(base_types(boss), vec!["Enemy"]);
    let extends: Vec<_> = result
        .structured_pending_relationships
        .iter()
        .filter(|p| p.pending.kind == RelationshipKind::Extends)
        .collect();
    assert_eq!(extends.len(), 1);
    assert_eq!(extends[0].target.terminal_name, "Enemy");
    assert_eq!(extends[0].pending.from_symbol_id, boss.id);
    let defeated = symbol(&result, "defeated", SymbolKind::Event);
    assert_eq!(defeated.parent_id.as_ref(), Some(&boss.id));
}

#[test]
fn gut_and_gdunit_suites_get_container_case_and_lifecycle_roles() {
    let gut = extract(
        "src/test_boss.gd",
        "class_name TestBoss extends GutTest\nfunc before_each() -> void:\n\tpass\nfunc test_boss_enrages() -> void:\n\tassert_true(true)\nclass TestStats:\n\textends GutTest\n\tfunc test_jump():\n\t\tpass\n",
    );
    assert_eq!(
        test_role(symbol(&gut, "TestBoss", SymbolKind::Class)),
        "test_container"
    );
    assert_eq!(
        test_role(symbol(&gut, "TestStats", SymbolKind::Class)),
        "test_container"
    );
    assert_eq!(
        test_role(symbol(&gut, "test_boss_enrages", SymbolKind::Method)),
        "test_case"
    );
    assert_eq!(
        test_role(symbol(&gut, "test_jump", SymbolKind::Method)),
        "test_case"
    );
    assert_eq!(
        test_role(symbol(&gut, "before_each", SymbolKind::Method)),
        "fixture_setup"
    );

    let gdunit = extract(
        "src/player/player_test.gd",
        "extends GdUnitTestSuite\n\nfunc before() -> void:\n\tpass\n\nfunc before_test() -> void:\n\tpass\n\nfunc after_test() -> void:\n\tpass\n\nfunc after() -> void:\n\tpass\n\nfunc test_player_moves() -> void:\n\tassert_bool(true).is_true()\n",
    );
    assert_eq!(
        test_role(symbol(&gdunit, "player_test", SymbolKind::Class)),
        "test_container"
    );
    for (name, role) in [
        ("before", "fixture_setup"),
        ("before_test", "fixture_setup"),
        ("after_test", "fixture_teardown"),
        ("after", "fixture_teardown"),
        ("test_player_moves", "test_case"),
    ] {
        assert_eq!(
            test_role(symbol(&gdunit, name, SymbolKind::Method)),
            role,
            "{name}"
        );
    }
}

#[test]
fn every_call_and_interior_member_in_a_chain_gets_its_own_rows() {
    let code = "extends Node\nfunc run() -> void:\n\tget_node(\"HUD\").get_child(0).queue_free()\n\tvar h = player.stats.health\n\treturn player.stats.summary().length()\n";
    let result = extract("chains.gd", code);
    let idents = |kind: IdentifierKind| -> Vec<String> {
        result
            .identifiers
            .iter()
            .filter(|i| i.kind == kind)
            .map(|i| i.name.clone())
            .collect()
    };
    let calls = idents(IdentifierKind::Call);
    for name in ["get_node", "get_child", "queue_free", "summary", "length"] {
        assert!(calls.contains(&name.to_string()), "{name}: {calls:?}");
    }
    let members = idents(IdentifierKind::MemberAccess);
    assert_eq!(
        members.iter().filter(|m| *m == "stats").count(),
        2,
        "{members:?}"
    );
    assert!(members.contains(&"health".to_string()));
    let receivers = pending_receivers(&result);
    for expected in [
        ("get_child", "get_node(\"HUD\")"),
        ("queue_free", "get_node(\"HUD\").get_child(0)"),
        ("summary", "stats"),
        ("length", "player.stats.summary()"),
    ] {
        assert!(
            receivers.contains(&(expected.0.to_string(), expected.1.to_string())),
            "{expected:?}: {receivers:?}"
        );
    }
}

#[test]
fn pending_receiver_stops_before_the_call_arguments() {
    let code = "class_name Fighter\nextends Node\n\nfunc fight(enemy, stats) -> void:\n\tenemy.take_damage(stats.power)\n\tself.log_event(Vector2(1.5, 2.0))\n\tFileAccess.open(PATH, FileAccess.WRITE)\n";
    let result = extract("fighter.gd", code);
    let receivers = pending_receivers(&result);
    for expected in [
        ("take_damage", "enemy"),
        ("log_event", "self"),
        ("open", "FileAccess"),
    ] {
        assert!(
            receivers.contains(&(expected.0.to_string(), expected.1.to_string())),
            "{expected:?}: {receivers:?}"
        );
    }
}

#[test]
fn constructor_initializer_and_accessor_bodies_are_callers() {
    let code = "class_name Spawner\nextends Node\n\n@onready var timer: Timer = get_node(\"Timer\")\nvar pool := ObjectPool.new(16)\nvar health: int = 100:\n\tset(value):\n\t\thealth = clampi(value, 0, 100)\n\t\trefresh_ui()\n\tget:\n\t\treturn compute()\nvar ratio: float:\n\tget = get_ratio, set = set_ratio\n\nfunc _init(count: int) -> void:\n\tsetup(count)\n\tLogger.info(\"spawner created\")\n\nfunc setup(count: int) -> void:\n\tpass\n\nfunc refresh_ui() -> void:\n\tpass\n\nfunc compute() -> int:\n\treturn 1\n\nfunc get_ratio() -> float:\n\treturn 1.0\n\nfunc set_ratio(v: float) -> void:\n\tpass\n";
    let result = extract("spawner.gd", code);
    assert_eq!(calls_from(&result, "_init"), vec!["setup", "pending:info"]);
    let mut health = calls_from(&result, "health");
    health.sort();
    assert_eq!(health, vec!["compute", "pending:clampi", "refresh_ui"]);
    let mut ratio = calls_from(&result, "ratio");
    ratio.sort();
    assert_eq!(ratio, vec!["get_ratio", "set_ratio"]);
    assert_eq!(calls_from(&result, "timer"), vec!["pending:get_node"]);
    assert_eq!(calls_from(&result, "pool"), vec!["pending:new"]);
    let refresh_call = result
        .identifiers
        .iter()
        .find(|i| i.name == "refresh_ui" && i.kind == IdentifierKind::Call)
        .unwrap();
    assert_eq!(
        name_of(&result, refresh_call.containing_symbol_id.as_ref()),
        "health"
    );
    let value = result
        .symbols
        .iter()
        .find(|s| s.name == "value")
        .map(|s| s.kind.clone());
    assert_ne!(value, Some(SymbolKind::Field));
}

#[test]
fn member_doc_comments_attach_only_to_their_own_block() {
    let code = "extends Node\n## Manages the score for a round.\n##\n## Tracks points.\n\n## Emitted when the score changes.\nsignal score_changed(value: int)\n\n## Max combo multiplier.\nconst MAX_COMBO := 8\n\n## The current score.\nvar score: int = 0\n\n## Tunable points per hit.\n@export var points_per_hit := 10\n\nenum Flags {\n\t## The first flag.\n\tFLAG_A,\n}\n\nclass Entry:\n\t## Player name.\n\tvar name: String\n";
    let result = extract("score.gd", code);
    let doc = |name: &str, kind: SymbolKind| symbol(&result, name, kind).doc_comment.clone();
    assert_eq!(
        doc("score", SymbolKind::Class).as_deref(),
        Some("## Manages the score for a round.\n##\n## Tracks points.")
    );
    assert_eq!(
        doc("score_changed", SymbolKind::Event).as_deref(),
        Some("## Emitted when the score changes.")
    );
    assert_eq!(
        doc("MAX_COMBO", SymbolKind::Constant).as_deref(),
        Some("## Max combo multiplier.")
    );
    assert_eq!(
        doc("score", SymbolKind::Field).as_deref(),
        Some("## The current score.")
    );
    assert_eq!(
        doc("points_per_hit", SymbolKind::Field).as_deref(),
        Some("## Tunable points per hit.")
    );
    assert_eq!(
        doc("FLAG_A", SymbolKind::EnumMember).as_deref(),
        Some("## The first flag.")
    );
    assert_eq!(
        doc("name", SymbolKind::Field).as_deref(),
        Some("## Player name.")
    );
}
