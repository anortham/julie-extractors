use crate::base::{ExtractionResults, RelationshipKind, SymbolKind};
use crate::extract_canonical;
use std::path::Path;

fn extract(path: &str, source: &str) -> ExtractionResults {
    extract_canonical(path, source, Path::new("/tmp/test")).expect("extraction succeeds")
}

fn symbol_name(results: &ExtractionResults, id: &str) -> String {
    results
        .symbols
        .iter()
        .find(|symbol| symbol.id == id)
        .map(|symbol| symbol.name.clone())
        .unwrap_or_else(|| format!("<{id}>"))
}

fn pending_rows(results: &ExtractionResults) -> Vec<String> {
    results
        .structured_pending_relationships
        .iter()
        .map(|pending| {
            format!(
                "{:?} {} -> {} recv={} imp={} rtype={}",
                pending.pending.kind,
                symbol_name(results, &pending.pending.from_symbol_id),
                pending.target.display_name,
                pending.target.receiver.as_deref().unwrap_or(""),
                pending.target.import_context.as_deref().unwrap_or(""),
                pending.receiver_type.as_deref().unwrap_or(""),
            )
        })
        .collect()
}

fn relationship_rows(results: &ExtractionResults) -> Vec<String> {
    results
        .relationships
        .iter()
        .map(|relationship| {
            format!(
                "{:?} {} -> {}",
                relationship.kind,
                symbol_name(results, &relationship.from_symbol_id),
                symbol_name(results, &relationship.to_symbol_id),
            )
        })
        .collect()
}

fn has_row(rows: &[String], expected: &str) -> bool {
    rows.iter().any(|row| row == expected)
}

#[test]
fn abstract_class_and_abstract_members_are_symbols_with_heritage() {
    let source = r#"import { Entity } from './entity';
/** Shape base. */
export abstract class Shape extends Entity implements Drawable {
  /** Area. */
  abstract area(): number;
  protected abstract readonly kind: string;
  describe(): string { return `${this.kind}: ${this.area()}`; }
}
export class Circle extends Shape { area(): number { return 3; } }
interface Drawable { draw(): void }
class C extends Repository<User> {}
"#;
    let results = extract("src/abstract.ts", source);
    let shape = results
        .symbols
        .iter()
        .find(|symbol| symbol.name == "Shape" && symbol.kind == SymbolKind::Class)
        .expect("abstract class Shape is a class symbol");
    assert_eq!(shape.doc_comment.as_deref(), Some("/** Shape base. */"));
    assert_eq!(
        shape.metadata.as_ref().unwrap()["isAbstract"],
        serde_json::json!(true)
    );
    assert_eq!(
        shape.signature.as_deref(),
        Some("abstract class Shape extends Entity implements Drawable")
    );

    let area = results
        .symbols
        .iter()
        .find(|symbol| symbol.name == "area" && symbol.parent_id.as_deref() == Some(&shape.id))
        .expect("abstract method area is parented to Shape");
    assert_eq!(area.kind, SymbolKind::Method);
    assert_eq!(area.doc_comment.as_deref(), Some("/** Area. */"));
    for member in ["kind", "describe"] {
        assert!(
            results
                .symbols
                .iter()
                .any(|symbol| symbol.name == member
                    && symbol.parent_id.as_deref() == Some(&shape.id)),
            "{member} must be parented to Shape"
        );
    }

    let relationships = relationship_rows(&results);
    for expected in [
        "Extends Circle -> Shape",
        "Implements Shape -> Drawable",
        "Calls describe -> area",
    ] {
        assert!(
            has_row(&relationships, expected),
            "{expected}: {relationships:#?}"
        );
    }
    let pending = pending_rows(&results);
    assert!(
        has_row(&pending, "Extends Shape -> Entity recv= imp= rtype="),
        "{pending:#?}"
    );
    assert!(
        !pending.iter().any(|row| row.starts_with("Extends Circle")),
        "{pending:#?}"
    );

    let generic_child = results
        .symbols
        .iter()
        .find(|symbol| symbol.name == "C" && symbol.kind == SymbolKind::Class)
        .unwrap();
    assert_eq!(
        generic_child.signature.as_deref(),
        Some("class C extends Repository<User>")
    );
    assert_eq!(
        generic_child.metadata.as_ref().unwrap()["extends"],
        serde_json::json!("Repository<User>")
    );
}

#[test]
fn enum_members_with_initializers_and_string_names_are_extracted() {
    let source = r#"enum Color {
  /** Red. */
  Red,
  Green,
  /** Blue. */
  Blue = 5,
  "quoted-key" = 6,
}
export enum Status { Active = "active", Disabled = "disabled" }
"#;
    let results = extract("src/enums.ts", source);
    let members: Vec<(String, String)> = results
        .symbols
        .iter()
        .filter(|symbol| symbol.kind == SymbolKind::EnumMember)
        .map(|symbol| {
            (
                symbol.name.clone(),
                symbol_name(&results, symbol.parent_id.as_deref().unwrap()),
            )
        })
        .collect();
    let expected: Vec<(String, String)> = [
        ("Red", "Color"),
        ("Green", "Color"),
        ("Blue", "Color"),
        ("quoted-key", "Color"),
        ("Active", "Status"),
        ("Disabled", "Status"),
    ]
    .iter()
    .map(|(name, parent)| (name.to_string(), parent.to_string()))
    .collect();
    assert_eq!(members, expected);
    let blue = results
        .symbols
        .iter()
        .find(|symbol| symbol.name == "Blue")
        .unwrap();
    assert_eq!(blue.doc_comment.as_deref(), Some("/** Blue. */"));
}

#[test]
fn doc_comments_attach_to_exported_and_const_declarations() {
    let source = r#"/** Plain const doc. */
const plain = 1;
/** Arrow doc. */
const arrow = () => 1;
/** Exported arrow doc. */
export const exportedArrow = () => 1;
/** Exported function doc. */
export function exportedFn() {}
/** Exported class doc. */
export class ExportedClass {}
/** Exported interface doc. */
export interface ExportedIface {}
/** Exported type doc. */
export type ExportedType = string;
/** Default doc. */
export default function defaultFn() {}
"#;
    let results = extract("src/docs.ts", source);
    for (name, kind, doc) in [
        ("plain", SymbolKind::Variable, "/** Plain const doc. */"),
        ("arrow", SymbolKind::Function, "/** Arrow doc. */"),
        (
            "exportedArrow",
            SymbolKind::Function,
            "/** Exported arrow doc. */",
        ),
        (
            "exportedFn",
            SymbolKind::Function,
            "/** Exported function doc. */",
        ),
        (
            "ExportedClass",
            SymbolKind::Class,
            "/** Exported class doc. */",
        ),
        (
            "ExportedIface",
            SymbolKind::Interface,
            "/** Exported interface doc. */",
        ),
        (
            "ExportedType",
            SymbolKind::Type,
            "/** Exported type doc. */",
        ),
        ("defaultFn", SymbolKind::Function, "/** Default doc. */"),
    ] {
        let symbol = results
            .symbols
            .iter()
            .find(|symbol| symbol.name == name && symbol.kind == kind)
            .unwrap_or_else(|| panic!("{name} {kind:?} missing"));
        assert_eq!(symbol.doc_comment.as_deref(), Some(doc), "{name}");
    }

    let documented: Vec<(u32, String)> = results
        .source_regions
        .iter()
        .filter(|region| region.kind.as_str() == "doc_comment")
        .map(|region| {
            (
                region.start_line,
                symbol_name(
                    &results,
                    region.containing_symbol_id.as_deref().unwrap_or(""),
                ),
            )
        })
        .collect();
    let expected: Vec<(u32, String)> = [
        (1, "plain"),
        (3, "arrow"),
        (5, "exportedArrow"),
        (7, "exportedFn"),
        (9, "ExportedClass"),
        (11, "ExportedIface"),
        (13, "ExportedType"),
        (15, "defaultFn"),
    ]
    .iter()
    .map(|(line, name)| (*line, name.to_string()))
    .collect();
    assert_eq!(documented, expected);
}

#[test]
fn member_calls_on_typed_receivers_emit_pending_rows() {
    let source = r#"import { BaseRepo } from './base';
import { Logger } from './logger';
export class UserRepo extends BaseRepo {
  constructor(private readonly logger: Logger) { super(); }
  load(id: string) {
    this.findById(id);
    super.save(id);
    this.logger.warn("x");
    const l: Logger = this.logger;
    l.warn("y");
    this.local();
  }
  local() {}
}
export function standalone(repo: UserRepo, log: Logger) {
  repo.load("1");
  log.warn("z");
  expect(repo).toBe(1);
  console.log("x");
}
"#;
    let results = extract("src/inherit.ts", source);
    let pending = pending_rows(&results);
    for expected in [
        "Calls load -> this.findById recv=this imp= rtype=UserRepo",
        "Calls load -> super.save recv=super imp= rtype=BaseRepo",
        "Calls load -> logger.warn recv=logger imp= rtype=",
        "Calls load -> l.warn recv=l imp= rtype=",
        "Calls standalone -> repo.load recv=repo imp= rtype=",
        "Calls standalone -> log.warn recv=log imp= rtype=",
    ] {
        assert!(has_row(&pending, expected), "{expected}: {pending:#?}");
    }
    for absent in ["local", "toBe", "console.log"] {
        assert!(
            !pending.iter().any(|row| row.contains(absent)),
            "{absent}: {pending:#?}"
        );
    }
    let relationships = relationship_rows(&results);
    assert!(
        has_row(&relationships, "Calls load -> local"),
        "{relationships:#?}"
    );
}

#[test]
fn member_calls_are_not_suppressed_by_same_named_file_symbols() {
    let source = r#"import { UsersService } from './users.service';
import * as api from './api';
export class Local {
  findAll() {}
  fetchUser() {}
}
export function run() {
  const svc = new UsersService();
  svc.findAll();
  api.fetchUser(1);
}
"#;
    let results = extract("src/collide.ts", source);
    let pending = pending_rows(&results);
    for expected in [
        "Calls run -> svc.findAll recv=svc imp=UsersService rtype=",
        "Calls run -> api.fetchUser recv=api imp=api rtype=",
    ] {
        assert!(has_row(&pending, expected), "{expected}: {pending:#?}");
    }
}

#[test]
fn jsx_component_usage_emits_calls_and_pending_rows() {
    let source = r#"import { UserCard } from './UserCard';
import * as UI from './ui';
import React from 'react';
function Badge({ label }: { label: string }) { return <span>{label}</span>; }
export const Profile = () => (
  <UI.Panel title="Profile">
    <UserCard />
    <Badge label="new" />
    <div />
  </UI.Panel>
);
export default function App() { return <Profile />; }
class Legacy extends React.Component { render() { return <Badge label="x" />; } }
"#;
    let results = extract("src/comp.tsx", source);
    let relationships = relationship_rows(&results);
    for expected in [
        "Calls Profile -> Badge",
        "Calls App -> Profile",
        "Calls render -> Badge",
    ] {
        assert!(
            has_row(&relationships, expected),
            "{expected}: {relationships:#?}"
        );
    }
    let pending = pending_rows(&results);
    for expected in [
        "Calls Profile -> UserCard recv= imp=UserCard rtype=",
        "Calls Profile -> UI.Panel recv=UI imp=UI rtype=",
    ] {
        assert!(has_row(&pending, expected), "{expected}: {pending:#?}");
    }
    assert!(
        !pending
            .iter()
            .chain(relationships.iter())
            .any(|row| row.contains("span") || row.contains("div")),
        "intrinsic elements emit nothing: {pending:#?}"
    );
    assert!(
        results
            .relationships
            .iter()
            .all(|relationship| relationship.kind != RelationshipKind::Calls
                || symbol_name(&results, &relationship.to_symbol_id) != "UserCard")
    );
}

#[test]
fn express_routes_detected_for_exported_typed_and_multiline_receivers() {
    let source = r#"import express, { Router } from "express";
export const app = express();
app.get("/healthz", (_req, res) => res.json({ ok: true }));
const api: Router = express.Router();
api.get("/users", (_req, res) => res.json([]));
const items = express.Router();
items
  .route("/:id")
  .get((_req, res) => res.json(1))
  .delete((_req, res) => res.sendStatus(204));
"#;
    let results = extract("src/server.ts", source);
    let mut routes: Vec<String> = results
        .structural_facts
        .iter()
        .filter(|fact| fact.pattern_id == "express.route.v1")
        .map(|fact| {
            let metadata = fact.metadata.clone().unwrap_or_default();
            format!(
                "{} {}",
                metadata.get("verb").and_then(|v| v.as_str()).unwrap_or(""),
                metadata
                    .get("route_template")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
            )
        })
        .collect();
    routes.sort();
    assert_eq!(
        routes,
        vec!["DELETE /:id", "GET /:id", "GET /healthz", "GET /users"]
    );
}

#[test]
fn fastify_routes_detected_for_exported_receiver_with_options() {
    let source = r#"import fastify from "fastify";
export const server = fastify({ logger: true });
server.get("/ping", async () => "pong");
"#;
    let results = extract("src/server.ts", source);
    assert!(
        results
            .structural_facts
            .iter()
            .any(|fact| fact.pattern_id == "fastify.route.v1"),
        "{:#?}",
        results.structural_facts
    );
}
