use crate::base::{
    ExtractionResults, IdentifierKind, RelationshipKind, StructuralFact, Symbol, SymbolKind,
    Visibility,
};
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
        .unwrap_or_else(|| panic!("no {kind:?} {name}"))
}

fn name_of(result: &ExtractionResults, id: &str) -> String {
    result
        .symbols
        .iter()
        .find(|s| s.id == id)
        .map(|s| s.name.clone())
        .unwrap_or_default()
}

fn annotation_keys(symbol: &Symbol) -> Vec<String> {
    symbol
        .annotations
        .iter()
        .map(|a| a.annotation_key.clone())
        .collect()
}

fn identifiers(result: &ExtractionResults, kind: IdentifierKind) -> Vec<String> {
    result
        .identifiers
        .iter()
        .filter(|i| i.kind == kind)
        .map(|i| format!("{}:{}", i.start_line, i.name))
        .collect()
}

fn resolved_type(result: &ExtractionResults, symbol: &Symbol) -> Option<String> {
    result
        .types
        .get(&symbol.id)
        .map(|t| t.resolved_type.clone())
}

fn facts<'a>(result: &'a ExtractionResults, pattern_id: &str) -> Vec<&'a StructuralFact> {
    result
        .structural_facts
        .iter()
        .filter(|f| f.pattern_id == pattern_id)
        .collect()
}

fn fact_lines(result: &ExtractionResults, pattern_id: &str, keys: &[&str]) -> Vec<String> {
    facts(result, pattern_id)
        .into_iter()
        .map(|fact| {
            let metadata = fact.metadata.clone().unwrap_or_default();
            let values: Vec<String> = keys
                .iter()
                .map(|key| match metadata.get(*key) {
                    Some(serde_json::Value::String(text)) => text.clone(),
                    Some(value) => value.to_string(),
                    None => "-".to_string(),
                })
                .collect();
            format!("{} {}", fact.start_line, values.join(" "))
        })
        .collect()
}

#[test]
fn members_follow_the_underscore_rule_and_locals_have_no_visibility() {
    let code = "class_name Vis\n\nvar health := 10\nvar _cache := {}\nconst _SECRET := 1\nconst LIMIT := 2\nsignal _internal_tick\nsignal hit\nenum _Hidden { A }\nclass _Inner:\n\tpass\n@export var _tuned := 1\n\nfunc shoot() -> void:\n\tvar bullet := 1\n\tconst LOCAL := 2\n";
    let result = extract("vis.gd", code);
    for (name, kind, visibility) in [
        ("health", SymbolKind::Field, Some(Visibility::Public)),
        ("_cache", SymbolKind::Field, Some(Visibility::Private)),
        ("_SECRET", SymbolKind::Constant, Some(Visibility::Private)),
        ("LIMIT", SymbolKind::Constant, Some(Visibility::Public)),
        (
            "_internal_tick",
            SymbolKind::Event,
            Some(Visibility::Private),
        ),
        ("hit", SymbolKind::Event, Some(Visibility::Public)),
        ("_Hidden", SymbolKind::Enum, Some(Visibility::Private)),
        ("_Inner", SymbolKind::Class, Some(Visibility::Private)),
        ("_tuned", SymbolKind::Field, Some(Visibility::Private)),
        ("bullet", SymbolKind::Variable, None),
        ("LOCAL", SymbolKind::Variable, None),
    ] {
        assert_eq!(symbol(&result, name, kind).visibility, visibility, "{name}");
    }
    let tuned = symbol(&result, "_tuned", SymbolKind::Field);
    assert_eq!(
        tuned.metadata.as_ref().and_then(|m| m.get("isExported")),
        Some(&serde_json::json!(true))
    );
}

#[test]
fn standalone_annotations_attach_to_the_next_declaration_only() {
    let code = "@tool\n@icon(\"res://icon.svg\")\nextends EditorPlugin\n\nconst SpawnerNode = preload(\"res://addons/spawner/spawner.gd\")\nvar _dock: Control\n\n@warning_ignore(\"unused_private_class_variable\")\nconst __source = 'x'\nvar _ui: Control\n\n@rpc(\"authority\", \"call_remote\", \"unreliable\")\nfunc sync_position(pos: Vector2) -> void:\n\tposition = pos\n\n@warning_ignore(\"unused_parameter\")\nstatic func helper(x: int) -> int:\n\treturn 0\n";
    let result = extract("plugin.gd", code);
    let class = symbol(&result, "plugin", SymbolKind::Class);
    assert_eq!(annotation_keys(class), vec!["tool", "icon"]);
    assert!(annotation_keys(symbol(&result, "SpawnerNode", SymbolKind::Constant)).is_empty());
    assert!(annotation_keys(symbol(&result, "_dock", SymbolKind::Field)).is_empty());
    assert!(annotation_keys(symbol(&result, "_ui", SymbolKind::Field)).is_empty());
    assert_eq!(
        annotation_keys(symbol(&result, "__source", SymbolKind::Constant)),
        vec!["warning_ignore"]
    );
    let sync = symbol(&result, "sync_position", SymbolKind::Method);
    assert_eq!(annotation_keys(sync), vec!["rpc"]);
    assert_eq!(
        sync.signature.as_deref(),
        Some(
            "@rpc(\"authority\", \"call_remote\", \"unreliable\")\nfunc sync_position(pos: Vector2) -> void:"
        )
    );
    assert_eq!(
        annotation_keys(symbol(&result, "helper", SymbolKind::Method)),
        vec!["warning_ignore"]
    );
    assert_eq!(
        symbol(&result, "SpawnerNode", SymbolKind::Constant)
            .signature
            .as_deref(),
        Some("const SpawnerNode = preload(\"res://addons/spawner/spawner.gd\")")
    );
}

#[test]
fn script_and_function_annotations_are_symbol_annotations() {
    let code = "@abstract\nclass_name Shape\nextends RefCounted\n\n@abstract func area() -> float\n\nfunc perimeter() -> float:\n\treturn 0.0\n";
    let result = extract("shape.gd", code);
    assert_eq!(
        annotation_keys(symbol(&result, "Shape", SymbolKind::Class)),
        vec!["abstract"]
    );
    let area = symbol(&result, "area", SymbolKind::Method);
    assert_eq!(annotation_keys(area), vec!["abstract"]);
    assert!(area.body_span.is_none());
    assert!(annotation_keys(symbol(&result, "perimeter", SymbolKind::Method)).is_empty());
}

#[test]
fn casts_and_type_tests_are_type_usages_with_type_facts() {
    let code = "extends Node\n\n@onready var hurtbox := $Hurtbox as Area2D\n\nfunc handle(body: Node) -> void:\n\tvar enemy := body as Enemy\n\tvar level = load(\"res://l.tscn\") as PackedScene\n\tif body is Projectile:\n\t\t(body as Projectile).explode()\n\tenemy.hit()\n";
    let result = extract("hurt.gd", code);
    let types = identifiers(&result, IdentifierKind::TypeUsage);
    for expected in [
        "3:Area2D",
        "6:Enemy",
        "7:PackedScene",
        "8:Projectile",
        "9:Projectile",
    ] {
        assert!(
            types.contains(&expected.to_string()),
            "{expected}: {types:?}"
        );
    }
    let reads = identifiers(&result, IdentifierKind::VariableRef);
    for name in ["Area2D", "Enemy", "Projectile", "PackedScene"] {
        assert!(
            !reads.iter().any(|r| r.ends_with(&format!(":{name}"))),
            "{name}: {reads:?}"
        );
    }
    for (name, kind, expected) in [
        ("hurtbox", SymbolKind::Field, "Area2D"),
        ("enemy", SymbolKind::Variable, "Enemy"),
        ("level", SymbolKind::Variable, "PackedScene"),
    ] {
        let declared = symbol(&result, name, kind);
        assert_eq!(
            resolved_type(&result, declared).as_deref(),
            Some(expected),
            "{name}"
        );
        assert_eq!(
            declared.metadata.as_ref().and_then(|m| m.get("dataType")),
            Some(&serde_json::json!(expected)),
            "{name}"
        );
    }
}

#[test]
fn qualified_type_annotations_are_type_usages() {
    let code = "class_name Registry\n\nfunc lookup(key: StringName) -> Enemy.Kind:\n\tvar inner: Outer.Inner = Outer.Inner.new()\n\treturn inner.kind\n";
    let result = extract("registry.gd", code);
    let types = identifiers(&result, IdentifierKind::TypeUsage);
    for expected in ["3:Enemy", "3:Kind", "4:Outer", "4:Inner"] {
        assert!(
            types.contains(&expected.to_string()),
            "{expected}: {types:?}"
        );
    }
    let members = identifiers(&result, IdentifierKind::MemberAccess);
    assert!(!members.contains(&"3:Kind".to_string()), "{members:?}");
    let lookup = symbol(&result, "lookup", SymbolKind::Method);
    assert_eq!(
        resolved_type(&result, lookup).as_deref(),
        Some("Enemy.Kind")
    );
    let inner = symbol(&result, "inner", SymbolKind::Variable);
    assert_eq!(
        resolved_type(&result, inner).as_deref(),
        Some("Outer.Inner")
    );
}

#[test]
fn bare_calls_resolve_to_the_enclosing_class_method() {
    let code = "class_name Game\nextends Node\n\nfunc reset() -> void:\n\tpass\n\nfunc start() -> void:\n\treset()\n\nclass Board:\n\tfunc reset() -> void:\n\t\tpass\n\tfunc clear() -> void:\n\t\treset()\n";
    let result = extract("game.gd", code);
    let calls: Vec<String> = result
        .relationships
        .iter()
        .filter(|r| r.kind == RelationshipKind::Calls)
        .map(|r| {
            let target = result
                .symbols
                .iter()
                .find(|s| s.id == r.to_symbol_id)
                .unwrap();
            format!(
                "{}->{}.{}",
                name_of(&result, &r.from_symbol_id),
                name_of(&result, target.parent_id.as_deref().unwrap_or_default()),
                target.name
            )
        })
        .collect();
    assert!(
        calls.contains(&"start->Game.reset".to_string()),
        "{calls:?}"
    );
    assert!(
        calls.contains(&"clear->Board.reset".to_string()),
        "{calls:?}"
    );
    assert!(
        result.pending_relationships.is_empty(),
        "{:?}",
        result.pending_relationships
    );
}

#[test]
fn every_export_form_publishes_an_export_fact() {
    let code = "extends Node\n\n@export var speed: float = 200.0\n@export_range(0, 100) var max_health: int = 100\n@export_enum(\"A\", \"B\") var mode: int\n@onready var sprite = $Sprite\nexport(int) var legacy = 200\nexport var jump_height := 5.0\nonready var label = $Label\n";
    let result = extract("player.gd", code);
    assert_eq!(
        fact_lines(
            &result,
            "gdscript.export_annotation.v1",
            &["annotation_name", "exported_variable"]
        ),
        vec![
            "3 export speed",
            "4 export_range max_health",
            "5 export_enum mode",
            "7 export legacy",
            "8 export jump_height",
        ]
    );
    let label = symbol(&result, "label", SymbolKind::Field);
    assert_eq!(
        label.metadata.as_ref().and_then(|m| m.get("isOnReady")),
        Some(&serde_json::json!(true))
    );
}

#[test]
fn signal_connections_and_emissions_are_facts() {
    let code = "extends Node\n\nsignal died\n\nfunc _ready() -> void:\n\t$Button.pressed.connect(_on_button_pressed)\n\tdied.connect(_on_died.bind(1))\n\tGameState.score_changed.connect(func(v): print(v))\n\tconnect(\"died\", _on_died)\n\t$Timer.connect(\"timeout\", self, \"_on_timeout\")\n\n func _on_button_pressed() -> void:\n\tdied.emit()\n\temit_signal(\"died\")\n\t$Hud.emit_signal(\"refreshed\", 1)\n";
    let code = code.replace("\n func", "\nfunc");
    let result = extract("net.gd", &code);
    assert_eq!(
        fact_lines(
            &result,
            "godot.signal_connection.v1",
            &["signal_name", "emitter", "handler"]
        ),
        vec![
            "6 pressed $Button _on_button_pressed",
            "7 died - _on_died",
            "8 score_changed GameState -",
            "9 died - _on_died",
            "10 timeout $Timer _on_timeout",
        ]
    );
    assert_eq!(
        fact_lines(
            &result,
            "godot.signal_emission.v1",
            &["signal_name", "emitter"]
        ),
        vec!["13 died -", "14 died -", "15 refreshed $Hud"]
    );
    for fact in facts(&result, "godot.signal_connection.v1") {
        let owner = fact
            .containing_symbol_id
            .as_deref()
            .map(|id| name_of(&result, id));
        assert_eq!(owner.as_deref(), Some("_ready"));
    }
}

#[test]
fn preload_load_and_script_extends_are_resource_references() {
    let code = "extends \"res://base/actor.gd\"\n\nconst Bullet = preload(\"res://scenes/bullet.tscn\")\nconst EnemyScript := preload(\"res://enemies/enemy.gd\")\n\nfunc spawn() -> void:\n\tvar level: PackedScene = load(\"res://levels/level_1.tscn\")\n\tvar cfg = ResourceLoader.load(\"res://data/config.tres\")\n\tvar cast := load(\"res://levels/cast.tscn\") as PackedScene\n\tadd_child(load(path_for()).instantiate())\n";
    let result = extract("spawner.gd", code);
    assert_eq!(
        fact_lines(
            &result,
            "godot.resource_reference.v1",
            &["resource_path", "loader", "bound_name"]
        ),
        vec![
            "1 res://base/actor.gd extends -",
            "3 res://scenes/bullet.tscn preload Bullet",
            "4 res://enemies/enemy.gd preload EnemyScript",
            "7 res://levels/level_1.tscn load level",
            "8 res://data/config.tres ResourceLoader.load cfg",
            "9 res://levels/cast.tscn load cast",
        ]
    );
    for fact in facts(&result, "godot.resource_reference.v1") {
        assert_eq!(
            fact.metadata.as_ref().and_then(|m| m.get("query_family")),
            Some(&serde_json::json!("imports"))
        );
    }
}

#[test]
fn node_paths_and_rpc_annotations_are_facts() {
    let code = "extends Node\n\n@onready var sprite: AnimatedSprite2D = $AnimatedSprite2D\n@onready var camera := %Camera\n@onready var hud = $\"UI/Hud Panel\"\n@onready var bar = get_node(\"UI/Bar\")\n\n@rpc(\"any_peer\", \"call_local\", \"reliable\")\nfunc sync_score(value: int) -> void:\n\tscore = value\n";
    let result = extract("game_state.gd", code);
    assert_eq!(
        fact_lines(&result, "godot.node_path.v1", &["node_path", "access"]),
        vec![
            "3 AnimatedSprite2D dollar",
            "4 Camera unique_name",
            "5 UI/Hud Panel dollar",
            "6 UI/Bar get_node",
        ]
    );
    assert_eq!(
        fact_lines(
            &result,
            "godot.rpc_annotation.v1",
            &["function_name", "rpc_arguments"]
        ),
        vec!["8 sync_score [\"any_peer\",\"call_local\",\"reliable\"]"]
    );
}
