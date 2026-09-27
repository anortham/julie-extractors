use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;

use julie_extractors::{ExtractionLevel, extract_canonical_for_language_at, supported_languages};
use rusqlite::Connection;
use serde_json::Value;
use tempfile::TempDir;

type ExpectedReceiver = (
    &'static str,
    &'static str,
    &'static str,
    Option<&'static str>,
    usize,
);
type ExpectedAbsence = (&'static str, &'static str, usize);

struct LanguageCase {
    language: &'static str,
    fixture: &'static str,
    expected: &'static [ExpectedReceiver],
    absences: &'static [ExpectedAbsence],
    not_applicable: Option<&'static str>,
    identifier_language: Option<&'static str>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct ReceiverFact {
    path: String,
    name: String,
    kind: String,
    start_line: u32,
    start_column: u32,
    end_line: u32,
    end_column: u32,
    start_byte: u32,
    end_byte: u32,
    receiver: String,
    qualifier: Option<String>,
}

struct ArtifactIdentifier {
    path: String,
    language: String,
    name: String,
    kind: String,
    start_line: u32,
    start_column: u32,
    end_line: u32,
    end_column: u32,
    start_byte: u32,
    end_byte: u32,
    receiver: Option<String>,
    qualifier: Option<String>,
    receiver_key_present: bool,
    qualifier_key_present: bool,
    reference_site_span: (
        Option<i64>,
        Option<i64>,
        Option<i64>,
        Option<i64>,
        Option<i64>,
        Option<i64>,
    ),
    reference_site_exact: bool,
}

const NO_EXPECTED_RECEIVERS: &[ExpectedReceiver] = &[];
const NO_EXPECTED_ABSENCES: &[ExpectedAbsence] = &[];

const LANGUAGE_CASES: &[LanguageCase] = &[
    LanguageCase {
        language: "rust",
        fixture: "fixtures/extraction/rust/basic/source.rs",
        expected: &[("self.mark()", "mark", "self", None, 1)],
        absences: &[
            ("client.fetch().await.unwrap()", "unwrap", 1),
            ("handle.await.len()", "len", 1),
        ],
        not_applicable: None,
        identifier_language: None,
    },
    LanguageCase {
        language: "c",
        fixture: "fixtures/extraction/c/basic/source.c",
        expected: &[("worker->id", "id", "worker", None, 1)],
        absences: NO_EXPECTED_ABSENCES,
        not_applicable: None,
        identifier_language: None,
    },
    LanguageCase {
        language: "cpp",
        fixture: "fixtures/extraction/cpp/basic/source.cpp",
        expected: &[("this->helper(id_)", "helper", "this", None, 1)],
        absences: NO_EXPECTED_ABSENCES,
        not_applicable: None,
        identifier_language: None,
    },
    LanguageCase {
        language: "go",
        fixture: "fixtures/extraction/go/basic/source.go",
        expected: &[("w.Run()", "Run", "w", None, 1)],
        absences: NO_EXPECTED_ABSENCES,
        not_applicable: None,
        identifier_language: None,
    },
    LanguageCase {
        language: "gomod",
        fixture: "fixtures/extraction/gomod/basic/go.mod",
        expected: NO_EXPECTED_RECEIVERS,
        absences: NO_EXPECTED_ABSENCES,
        not_applicable: Some(
            "module manifests declare module paths and dependencies, not object member expressions",
        ),
        identifier_language: None,
    },
    LanguageCase {
        language: "gosum",
        fixture: "fixtures/extraction/gosum/basic/go.sum",
        expected: NO_EXPECTED_RECEIVERS,
        absences: NO_EXPECTED_ABSENCES,
        not_applicable: Some(
            "checksum rows contain package versions and hashes, not executable references",
        ),
        identifier_language: None,
    },
    LanguageCase {
        language: "zig",
        fixture: "fixtures/extraction/zig/basic/source.zig",
        expected: &[
            ("self.run()", "run", "self", None, 1),
            ("!self.isOpen()", "isOpen", "self", None, 1),
        ],
        absences: &[(".items = 1", "items", 1)],
        not_applicable: None,
        identifier_language: None,
    },
    LanguageCase {
        language: "typescript",
        fixture: "fixtures/extraction/typescript/nextjs_route_handler/app/api/users/[id]/route.ts",
        expected: &[
            ("NextResponse.json", "json", "NextResponse", None, 1),
            ("params.id", "id", "params", None, 1),
        ],
        absences: NO_EXPECTED_ABSENCES,
        not_applicable: None,
        identifier_language: None,
    },
    LanguageCase {
        language: "tsx",
        fixture: "fixtures/extraction/tsx/basic/source.tsx",
        expected: &[("value.trim()", "trim", "value", None, 1)],
        absences: NO_EXPECTED_ABSENCES,
        not_applicable: None,
        identifier_language: None,
    },
    LanguageCase {
        language: "javascript",
        fixture: "fixtures/extraction/javascript/basic/source.js",
        expected: &[("this.id", "id", "this", None, 2)],
        absences: NO_EXPECTED_ABSENCES,
        not_applicable: None,
        identifier_language: None,
    },
    LanguageCase {
        language: "jsx",
        fixture: "fixtures/extraction/jsx/basic/source.jsx",
        expected: &[("value.trim()", "trim", "value", None, 1)],
        absences: NO_EXPECTED_ABSENCES,
        not_applicable: None,
        identifier_language: None,
    },
    LanguageCase {
        language: "html",
        fixture: "fixtures/extraction/html/basic/source.html",
        expected: &[("this.render()", "render", "this", None, 1)],
        absences: NO_EXPECTED_ABSENCES,
        not_applicable: None,
        identifier_language: Some("javascript"),
    },
    LanguageCase {
        language: "css",
        fixture: "fixtures/extraction/css/basic/source.css",
        expected: NO_EXPECTED_RECEIVERS,
        absences: NO_EXPECTED_ABSENCES,
        not_applicable: Some(
            "selectors and CSS functions describe styles and custom properties, not object receivers",
        ),
        identifier_language: None,
    },
    LanguageCase {
        language: "vue",
        fixture: "fixtures/extraction/vue/basic/source.vue",
        expected: &[("value.trim()", "trim", "value", None, 1)],
        absences: NO_EXPECTED_ABSENCES,
        not_applicable: None,
        identifier_language: Some("vue"),
    },
    LanguageCase {
        language: "python",
        fixture: "fixtures/extraction/python/basic/source.py",
        expected: &[("self.id = id", "id", "self", None, 1)],
        absences: NO_EXPECTED_ABSENCES,
        not_applicable: None,
        identifier_language: None,
    },
    LanguageCase {
        language: "java",
        fixture: "fixtures/extraction/java/basic/source.java",
        expected: &[("stream.close()", "close", "stream", None, 1)],
        absences: NO_EXPECTED_ABSENCES,
        not_applicable: None,
        identifier_language: None,
    },
    LanguageCase {
        language: "csharp",
        fixture: "fixtures/extraction/csharp/basic/source.cs",
        expected: &[("GraphTraversal.Reach", "Reach", "GraphTraversal", None, 1)],
        absences: NO_EXPECTED_ABSENCES,
        not_applicable: None,
        identifier_language: None,
    },
    LanguageCase {
        language: "vbnet",
        fixture: "fixtures/extraction/vbnet/calls_and_bodies/source.vb",
        expected: &[
            (
                "System.Service\n                .Start()",
                "Start",
                "System.Service",
                None,
                1,
            ),
            ("o?.GetTotal()", "GetTotal", "o", None, 1),
        ],
        absences: NO_EXPECTED_ABSENCES,
        not_applicable: None,
        identifier_language: None,
    },
    LanguageCase {
        language: "php",
        fixture: "fixtures/extraction/php/basic/source.php",
        expected: &[
            ("$this->missingWave2()", "missingWave2", "$this", None, 1),
            (
                "\\Attribute::TARGET_CLASS",
                "TARGET_CLASS",
                "Attribute",
                None,
                1,
            ),
            (
                "#[\\Attribute(\\Attribute::TARGET_CLASS | \\Attribute::TARGET_METHOD | \\Attribute::TARGET_PROPERTY)]",
                "TARGET_METHOD",
                "Attribute",
                None,
                1,
            ),
            (
                "#[\\Attribute(\\Attribute::TARGET_CLASS | \\Attribute::TARGET_METHOD | \\Attribute::TARGET_PROPERTY)]",
                "TARGET_PROPERTY",
                "Attribute",
                None,
                1,
            ),
            (
                "#[\\Attribute(\\Attribute::TARGET_METHOD)]",
                "TARGET_METHOD",
                "Attribute",
                None,
                1,
            ),
            (
                "#[\\Attribute(\\Attribute::TARGET_PROPERTY)]",
                "TARGET_PROPERTY",
                "Attribute",
                None,
                1,
            ),
        ],
        absences: NO_EXPECTED_ABSENCES,
        not_applicable: None,
        identifier_language: None,
    },
    LanguageCase {
        language: "ruby",
        fixture: "fixtures/extraction/ruby/basic/source.rb",
        expected: &[
            ("self.helper", "helper", "self", None, 1),
            ("service&.run()", "run", "service", None, 1),
        ],
        absences: NO_EXPECTED_ABSENCES,
        not_applicable: None,
        identifier_language: None,
    },
    LanguageCase {
        language: "swift",
        fixture: "fixtures/extraction/swift/basic/source.swift",
        expected: &[("self.persist()", "persist", "self", None, 2)],
        absences: NO_EXPECTED_ABSENCES,
        not_applicable: None,
        identifier_language: None,
    },
    LanguageCase {
        language: "kotlin",
        fixture: "fixtures/extraction/kotlin/basic/source.kt",
        expected: &[("this.recordRun(id)", "recordRun", "this", None, 1)],
        absences: NO_EXPECTED_ABSENCES,
        not_applicable: None,
        identifier_language: None,
    },
    LanguageCase {
        language: "scala",
        fixture: "fixtures/extraction/scala/basic/source.scala",
        expected: &[("this.m()", "m", "this", None, 1)],
        absences: NO_EXPECTED_ABSENCES,
        not_applicable: None,
        identifier_language: None,
    },
    LanguageCase {
        language: "dart",
        fixture: "fixtures/extraction/dart/basic/source.dart",
        expected: &[
            ("this.persist()", "persist", "this", None, 1),
            ("service..run()", "run", "service", None, 1),
            ("service?..run()", "run", "service", None, 1),
        ],
        absences: NO_EXPECTED_ABSENCES,
        not_applicable: None,
        identifier_language: None,
    },
    LanguageCase {
        language: "elixir",
        fixture: "fixtures/extraction/elixir/basic/source.ex",
        expected: &[("Map.new()", "new", "Map", None, 1)],
        absences: NO_EXPECTED_ABSENCES,
        not_applicable: None,
        identifier_language: None,
    },
    LanguageCase {
        language: "fsharp",
        fixture: "fixtures/extraction/fsharp/basic/source.fs",
        expected: &[
            ("= this.Helper()", "Helper", "this", None, 1),
            (
                "System.Console.WriteLine(point.X)",
                "WriteLine",
                "Console",
                Some("System"),
                1,
            ),
        ],
        absences: &[("| Some value -> log value", "log", 1)],
        not_applicable: None,
        identifier_language: None,
    },
    LanguageCase {
        language: "erlang",
        fixture: "fixtures/extraction/erlang/basic/source.erl",
        expected: &[("maps:new()", "new", "maps", None, 1)],
        absences: NO_EXPECTED_ABSENCES,
        not_applicable: None,
        identifier_language: None,
    },
    LanguageCase {
        language: "lua",
        fixture: "fixtures/extraction/lua/basic/source.lua",
        expected: &[("canvas:clear()", "clear", "canvas", None, 1)],
        absences: NO_EXPECTED_ABSENCES,
        not_applicable: None,
        identifier_language: None,
    },
    LanguageCase {
        language: "qml",
        fixture: "fixtures/extraction/qml/basic/source.qml",
        expected: &[("root.format(root.title)", "format", "root", None, 1)],
        absences: NO_EXPECTED_ABSENCES,
        not_applicable: None,
        identifier_language: None,
    },
    LanguageCase {
        language: "qmldir",
        fixture: "fixtures/extraction/qmldir/basic/qmldir",
        expected: NO_EXPECTED_RECEIVERS,
        absences: NO_EXPECTED_ABSENCES,
        not_applicable: Some(
            "qmldir declares QML module metadata and file registrations, not executable object members",
        ),
        identifier_language: None,
    },
    LanguageCase {
        language: "r",
        fixture: "fixtures/extraction/r/idioms/source.R",
        expected: &[
            ("private$log_it(\"speak\")", "log_it", "private", None, 1),
            ("object@slot", "slot", "object", None, 1),
        ],
        absences: NO_EXPECTED_ABSENCES,
        not_applicable: None,
        identifier_language: None,
    },
    LanguageCase {
        language: "bash",
        fixture: "fixtures/extraction/bash/basic/source.sh",
        expected: NO_EXPECTED_RECEIVERS,
        absences: &[("trap cleanup EXIT", "trap", 1)],
        not_applicable: Some(
            "shell invocations are command names and arguments rather than object member calls",
        ),
        identifier_language: None,
    },
    LanguageCase {
        language: "powershell",
        fixture: "fixtures/extraction/powershell/basic/source.ps1",
        expected: &[("$Config.Run($null)", "Run", "Config", None, 1)],
        absences: &[("Get-Process | Select-Object -First 1", "Get-Process", 1)],
        not_applicable: None,
        identifier_language: None,
    },
    LanguageCase {
        language: "gdscript",
        fixture: "fixtures/extraction/gdscript/basic/source.gd",
        expected: &[("self.persist()", "persist", "self", None, 2)],
        absences: NO_EXPECTED_ABSENCES,
        not_applicable: None,
        identifier_language: None,
    },
    LanguageCase {
        language: "razor",
        fixture: "fixtures/extraction/razor/basic/source.razor",
        expected: &[
            ("this.Refresh()", "Refresh", "this", None, 1),
            ("@item.Price.ToString(\"C\")", "Price", "item", None, 1),
            (
                "@item.Price.ToString(\"C\")",
                "ToString",
                "Price",
                Some("item"),
                1,
            ),
        ],
        absences: NO_EXPECTED_ABSENCES,
        not_applicable: None,
        identifier_language: None,
    },
    LanguageCase {
        language: "sql",
        fixture: "fixtures/extraction/sql/basic/source.sql",
        expected: &[("SELECT w.id", "id", "w", None, 1)],
        absences: NO_EXPECTED_ABSENCES,
        not_applicable: None,
        identifier_language: None,
    },
    LanguageCase {
        language: "regex",
        fixture: "fixtures/extraction/regex/basic/source.regex",
        expected: NO_EXPECTED_RECEIVERS,
        absences: NO_EXPECTED_ABSENCES,
        not_applicable: Some(
            "regex groups and backreferences refer to pattern captures, not object members",
        ),
        identifier_language: None,
    },
    LanguageCase {
        language: "markdown",
        fixture: "fixtures/extraction/markdown/basic/source.md",
        expected: NO_EXPECTED_RECEIVERS,
        absences: NO_EXPECTED_ABSENCES,
        not_applicable: Some(
            "Markdown links and headings describe document targets and anchors, not object members",
        ),
        identifier_language: None,
    },
    LanguageCase {
        language: "json",
        fixture: "fixtures/extraction/json/basic/source.json",
        expected: NO_EXPECTED_RECEIVERS,
        absences: NO_EXPECTED_ABSENCES,
        not_applicable: Some(
            "JSON object keys and reference fields are data, not executable member accesses",
        ),
        identifier_language: None,
    },
    LanguageCase {
        language: "toml",
        fixture: "fixtures/extraction/toml/basic/source.toml",
        expected: NO_EXPECTED_RECEIVERS,
        absences: NO_EXPECTED_ABSENCES,
        not_applicable: Some(
            "TOML table paths and keys are configuration structure, not executable members",
        ),
        identifier_language: None,
    },
    LanguageCase {
        language: "yaml",
        fixture: "fixtures/extraction/yaml/basic/source.yaml",
        expected: NO_EXPECTED_RECEIVERS,
        absences: NO_EXPECTED_ABSENCES,
        not_applicable: Some(
            "YAML resource paths and configuration references do not express object member calls",
        ),
        identifier_language: None,
    },
    LanguageCase {
        language: "xml",
        fixture: "fixtures/extraction/xml/basic/source.xml",
        expected: NO_EXPECTED_RECEIVERS,
        absences: NO_EXPECTED_ABSENCES,
        not_applicable: Some(
            "XML element, attribute, and task relationships describe document structure, not object member expressions",
        ),
        identifier_language: None,
    },
];

const SUPPLEMENTAL_CASES: &[LanguageCase] = &[
    LanguageCase {
        language: "csharp",
        fixture: "fixtures/extraction/csharp/base_lists_and_calls/source.cs",
        expected: &[
            ("base.Save(item)", "Save", "base", None, 1),
            (
                "logger?.LogInformation(\"saved\")",
                "LogInformation",
                "logger",
                None,
                1,
            ),
            (
                "Changed?.Invoke(this, EventArgs.Empty)",
                "Invoke",
                "Changed",
                None,
                1,
            ),
            (
                "services.AddScoped<IFoo, Foo>()",
                "AddScoped",
                "services",
                None,
                1,
            ),
            ("other?.Run(null)", "Run", "other", None, 1),
            ("other?.Inner.Flush()", "Inner", "other", None, 1),
            ("other?.Inner.Flush()", "Flush", "Inner", Some("other"), 1),
        ],
        absences: NO_EXPECTED_ABSENCES,
        not_applicable: None,
        identifier_language: None,
    },
    LanguageCase {
        language: "csharp",
        fixture: "fixtures/extraction/csharp/csharp14/source.cs",
        expected: &[
            (
                "customer?.Order = GetCurrentOrder()",
                "Order",
                "customer",
                None,
                1,
            ),
            ("int.TryParse(text, out result)", "TryParse", "int", None, 1),
        ],
        absences: NO_EXPECTED_ABSENCES,
        not_applicable: None,
        identifier_language: None,
    },
    LanguageCase {
        language: "csharp",
        fixture: "fixtures/extraction/csharp/http_client/source.cs",
        expected: &[(
            "client.GetFromJsonAsync<User>(\"/api/users/1\")",
            "GetFromJsonAsync",
            "client",
            None,
            1,
        )],
        absences: NO_EXPECTED_ABSENCES,
        not_applicable: None,
        identifier_language: None,
    },
    LanguageCase {
        language: "csharp",
        fixture: "fixtures/extraction/csharp/language_idioms/source.cs",
        expected: &[
            (
                "app.MapHub<ChatHub>(\"/hubs/chat\")",
                "MapHub",
                "app",
                None,
                1,
            ),
            (
                "string.IsNullOrWhiteSpace(text)",
                "IsNullOrWhiteSpace",
                "string",
                None,
                1,
            ),
            (
                "modelBuilder.Entity<Order>()",
                "Entity",
                "modelBuilder",
                None,
                1,
            ),
            (
                "Database.SqlQueryRaw<int>(\"SELECT COUNT(*) FROM orders\")",
                "SqlQueryRaw",
                "Database",
                None,
                1,
            ),
        ],
        absences: NO_EXPECTED_ABSENCES,
        not_applicable: None,
        identifier_language: None,
    },
    LanguageCase {
        language: "razor",
        fixture: "fixtures/extraction/razor/attribute-expressions/explicit/source.razor",
        expected: &[
            ("string.Empty", "Empty", "string", None, 1),
            (
                "result?.UploadFailures",
                "UploadFailures",
                "result",
                None,
                1,
            ),
            ("result?.Refresh()", "Refresh", "result", None, 1),
        ],
        absences: NO_EXPECTED_ABSENCES,
        not_applicable: None,
        identifier_language: None,
    },
    LanguageCase {
        language: "dart",
        fixture: "fixtures/extraction/dart/structure/source.dart",
        expected: &[
            ("User.fromJson(row)", "fromJson", "User", None, 1),
            ("final other = new Cart.empty();", "empty", "Cart", None, 1),
        ],
        absences: NO_EXPECTED_ABSENCES,
        not_applicable: None,
        identifier_language: None,
    },
    LanguageCase {
        language: "dart",
        fixture: "fixtures/extraction/dart/test_roles/source.dart",
        expected: &[("cubit.increment()", "increment", "cubit", None, 1)],
        absences: NO_EXPECTED_ABSENCES,
        not_applicable: None,
        identifier_language: None,
    },
    LanguageCase {
        language: "elixir",
        fixture: "fixtures/extraction/elixir/http_client_deferred/source.ex",
        expected: &[
            (
                ":httpc.request(\"https://api.example.com/httpc\")",
                "request",
                "httpc",
                None,
                1,
            ),
            (
                ":httpc.request(:post, {'/httpc/items', []}, [], [])",
                "request",
                "httpc",
                None,
                1,
            ),
        ],
        absences: NO_EXPECTED_ABSENCES,
        not_applicable: None,
        identifier_language: None,
    },
    LanguageCase {
        language: "elixir",
        fixture: "fixtures/extraction/elixir/language_forms/source.ex",
        expected: &[
            ("cart.user.admin?", "admin?", "user", Some("cart"), 1),
            (":ets.lookup(:carts, cart.id)", "lookup", "ets", None, 1),
            (
                "Jason.Encode.map(circle, opts)",
                "map",
                "Encode",
                Some("Jason"),
                1,
            ),
        ],
        absences: NO_EXPECTED_ABSENCES,
        not_applicable: None,
        identifier_language: None,
    },
    LanguageCase {
        language: "elixir",
        fixture: "fixtures/extraction/elixir/module_calls/source.ex",
        expected: &[
            (
                "Mailer.Queue.push(config)",
                "push",
                "Queue",
                Some("Mailer"),
                1,
            ),
            (
                "Fixture.Accounts.total([seed])",
                "total",
                "Accounts",
                Some("Fixture"),
                1,
            ),
        ],
        absences: NO_EXPECTED_ABSENCES,
        not_applicable: None,
        identifier_language: None,
    },
    LanguageCase {
        language: "gdscript",
        fixture: "fixtures/extraction/gdscript/godot/source.gd",
        expected: &[
            (
                "$Hurtbox.area_entered.connect(_on_hurtbox_area_entered)",
                "area_entered",
                "$Hurtbox",
                None,
                1,
            ),
            (
                "$Hurtbox.area_entered.connect(_on_hurtbox_area_entered)",
                "connect",
                "area_entered",
                Some("$Hurtbox"),
                1,
            ),
            ("Outer.Inner.new()", "Inner", "Outer", None, 1),
        ],
        absences: NO_EXPECTED_ABSENCES,
        not_applicable: None,
        identifier_language: None,
    },
    LanguageCase {
        language: "gdscript",
        fixture: "fixtures/extraction/gdscript/structure/source.gd",
        expected: &[
            ("player.stats.health", "stats", "player", None, 1),
            ("player.stats.health", "health", "stats", Some("player"), 1),
            ("FileAccess.WRITE", "WRITE", "FileAccess", None, 1),
        ],
        absences: NO_EXPECTED_ABSENCES,
        not_applicable: None,
        identifier_language: None,
    },
    LanguageCase {
        language: "java",
        fixture: "fixtures/extraction/java/hierarchy_and_calls/source.java",
        expected: &[
            ("this::handle", "handle", "this", None, 1),
            ("User::getName", "getName", "User", None, 1),
            ("System.out::println", "println", "out", Some("System"), 1),
        ],
        absences: NO_EXPECTED_ABSENCES,
        not_applicable: None,
        identifier_language: None,
    },
    LanguageCase {
        language: "php",
        fixture: "fixtures/extraction/php/laravel_routes/source.php",
        expected: &[("self::LEGACY_PATH", "LEGACY_PATH", "self", None, 1)],
        absences: NO_EXPECTED_ABSENCES,
        not_applicable: None,
        identifier_language: None,
    },
    LanguageCase {
        language: "php",
        fixture: "fixtures/extraction/php/modern_members/source.php",
        expected: &[
            ("Status::Active", "Active", "Status", None, 1),
            ("self::ROLE", "ROLE", "self", None, 1),
        ],
        absences: NO_EXPECTED_ABSENCES,
        not_applicable: None,
        identifier_language: None,
    },
    LanguageCase {
        language: "php",
        fixture: "fixtures/extraction/php/wave2_semantics/source.php",
        expected: &[
            ("self::validate($cents)", "validate", "self", None, 1),
            ("static::validate($cents)", "validate", "static", None, 1),
            ("Money::validate($cents)", "validate", "Money", None, 1),
            (
                "\\DB::statement('VACUUM balances')",
                "statement",
                "DB",
                None,
                1,
            ),
        ],
        absences: NO_EXPECTED_ABSENCES,
        not_applicable: None,
        identifier_language: None,
    },
    LanguageCase {
        language: "python",
        fixture: "fixtures/extraction/python/wave2_semantics/source.py",
        expected: &[("Color.RED", "RED", "Color", None, 1)],
        absences: NO_EXPECTED_ABSENCES,
        not_applicable: None,
        identifier_language: None,
    },
    LanguageCase {
        language: "fsharp",
        fixture: "fixtures/extraction/fsharp/declaration_forms/source.fs",
        expected: &[(
            "new System.IO.MemoryStream()",
            "MemoryStream",
            "IO",
            Some("System"),
            1,
        )],
        absences: NO_EXPECTED_ABSENCES,
        not_applicable: None,
        identifier_language: None,
    },
    LanguageCase {
        language: "ruby",
        fixture: "fixtures/extraction/ruby/app_structure/source.rb",
        expected: &[("$stderr.puts e", "puts", "$stderr", None, 1)],
        absences: &[(
            "Mailer.with(to: owner).receipt.deliver_later",
            "deliver_later",
            1,
        )],
        not_applicable: None,
        identifier_language: None,
    },
    LanguageCase {
        language: "ruby",
        fixture: "fixtures/extraction/ruby/backend_http_boundaries/source.rb",
        expected: &[
            (
                "Rails.application.routes.draw do",
                "routes",
                "application",
                Some("Rails"),
                1,
            ),
            (
                "Rails.application.routes.draw do",
                "draw",
                "routes",
                Some("Rails.application"),
                1,
            ),
        ],
        absences: NO_EXPECTED_ABSENCES,
        not_applicable: None,
        identifier_language: None,
    },
    LanguageCase {
        language: "ruby",
        fixture: "fixtures/extraction/ruby/wave2_semantics/source.rb",
        expected: &[
            ("@ledger.sum", "sum", "@ledger", None, 1),
            ("@ledger.clear", "clear", "@ledger", None, 1),
            ("@ledger.record(counter)", "record", "@ledger", None, 1),
        ],
        absences: NO_EXPECTED_ABSENCES,
        not_applicable: None,
        identifier_language: None,
    },
    LanguageCase {
        language: "rust",
        fixture: "fixtures/extraction/rust/items_and_macros/source.rs",
        expected: &[
            ("Mutex::new(vec![])", "new", "Mutex", None, 1),
            ("RefCell::new(0)", "new", "RefCell", None, 1),
        ],
        absences: NO_EXPECTED_ABSENCES,
        not_applicable: None,
        identifier_language: None,
    },
    LanguageCase {
        language: "rust",
        fixture: "fixtures/extraction/rust/trait_impl/source.rs",
        expected: &[("self.run()", "run", "self", None, 1)],
        absences: NO_EXPECTED_ABSENCES,
        not_applicable: None,
        identifier_language: None,
    },
    LanguageCase {
        language: "sql",
        fixture: "fixtures/extraction/sql/pg_dump/source.sql",
        expected: &[
            (
                "public.recent_orders(now()::date)",
                "recent_orders",
                "public",
                None,
                1,
            ),
            (
                "EXECUTE FUNCTION public.touch();",
                "touch",
                "public",
                None,
                1,
            ),
        ],
        absences: NO_EXPECTED_ABSENCES,
        not_applicable: None,
        identifier_language: None,
    },
    LanguageCase {
        language: "sql",
        fixture: "fixtures/extraction/sql/tsql_routines/source.sql",
        expected: &[
            (
                "EXEC dbo.usp_WriteAudit N'order';",
                "usp_WriteAudit",
                "dbo",
                None,
                1,
            ),
            (
                "EXECUTE billing.usp_Charge @CustomerId, @Total;",
                "usp_Charge",
                "billing",
                None,
                1,
            ),
        ],
        absences: NO_EXPECTED_ABSENCES,
        not_applicable: None,
        identifier_language: None,
    },
    LanguageCase {
        language: "sql",
        fixture: "fixtures/extraction/sql/tsql_semantics/source.sql",
        expected: &[
            (
                "EXEC tSQLt.NewTestClass 'OrderTests';",
                "NewTestClass",
                "tSQLt",
                None,
                1,
            ),
            (
                "EXEC tSQLt.FakeTable 'dbo.Orders';",
                "FakeTable",
                "tSQLt",
                None,
                1,
            ),
            (
                "EXEC dbo.usp_Drain @Batch = 10;",
                "usp_Drain",
                "dbo",
                None,
                1,
            ),
            (
                "EXEC dbo.usp_WriteAudit @Entity = N'Order';",
                "usp_WriteAudit",
                "dbo",
                None,
                1,
            ),
        ],
        absences: NO_EXPECTED_ABSENCES,
        not_applicable: None,
        identifier_language: None,
    },
    LanguageCase {
        language: "swift",
        fixture: "fixtures/extraction/swift/declarations/source.swift",
        expected: &[("-m.cents", "cents", "m", None, 1)],
        absences: NO_EXPECTED_ABSENCES,
        not_applicable: None,
        identifier_language: None,
    },
    LanguageCase {
        language: "vbnet",
        fixture: "fixtures/extraction/vbnet/basic/source.vb",
        expected: &[
            ("Implements IJob.Run", "Run", "IJob", None, 1),
            ("Handles Button.Click", "Click", "Button", None, 1),
            ("Me.Run()", "Run", "Me", None, 1),
        ],
        absences: NO_EXPECTED_ABSENCES,
        not_applicable: None,
        identifier_language: None,
    },
    LanguageCase {
        language: "vbnet",
        fixture: "fixtures/extraction/vbnet/call_initializers/source.vb",
        expected: &[("Dim own = Me.Load()", "Load", "Me", None, 1)],
        absences: NO_EXPECTED_ABSENCES,
        not_applicable: None,
        identifier_language: None,
    },
    LanguageCase {
        language: "vbnet",
        fixture: "fixtures/extraction/vbnet/framework_and_scopes/source.vb",
        expected: &[
            (
                "Implements IOrderApi.Create",
                "Create",
                "IOrderApi",
                None,
                1,
            ),
            ("MyBase.Reset()", "Reset", "MyBase", None, 1),
            ("Handles _timer.Tick", "Tick", "_timer", None, 1),
        ],
        absences: &[("_repo.Find(orders.Count).Customer.Save(orders)", "Save", 1)],
        not_applicable: None,
        identifier_language: None,
    },
    LanguageCase {
        language: "zig",
        fixture: "fixtures/extraction/zig/web/source.zig",
        expected: &[
            ("httpz.Server(void)", "Server", "httpz", None, 1),
            (
                "fn getUser(req: *httpz.Request, res: *httpz.Response)",
                "Request",
                "httpz",
                None,
                1,
            ),
            (
                "fn getUser(req: *httpz.Request, res: *httpz.Response)",
                "Response",
                "httpz",
                None,
                1,
            ),
        ],
        absences: NO_EXPECTED_ABSENCES,
        not_applicable: None,
        identifier_language: None,
    },
];

const RECEIVER_EDGE_SOURCE: &str = r#"
const café = { client: { send() {} } };
const service = { send() {} };
const items = [service];
function factory() { return café; }
function standalone() {}
function receiverCases() {
  café /* receiver/member gap */ . client
    /* member/call gap */ . send();
  service.send();
  service.send();
  factory().client.send();
  items[0].send();
  standalone();
}
// docs.example.invalid/client.send()
// /api/v1/client.send()
"#;

const RECEIVER_EDGE_EXPECTATIONS: &[ExpectedReceiver] = &[
    (
        "café /* receiver/member gap */ . client\n    /* member/call gap */ . send()",
        "send",
        "client",
        Some("café"),
        1,
    ),
    ("service.send()", "send", "service", None, 2),
];

const RECEIVER_EDGE_ABSENCES: &[ExpectedAbsence] = &[
    ("factory().client.send()", "send", 1),
    ("items[0].send()", "send", 1),
    ("standalone();", "standalone", 1),
];
const RECEIVER_EDGE_UNEXTRACTED: &[ExpectedAbsence] = &[
    ("// docs.example.invalid/client.send()", "send", 1),
    ("// /api/v1/client.send()", "send", 1),
];
const EMBEDDED_NON_MEMBER_HOST_CONTENT: &str = "<!-- api.client.send() -->";

const RECEIVER_EDGE_CASE: LanguageCase = LanguageCase {
    language: "javascript",
    fixture: "",
    expected: RECEIVER_EDGE_EXPECTATIONS,
    absences: RECEIVER_EDGE_ABSENCES,
    not_applicable: None,
    identifier_language: None,
};

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

fn prepare_source(case: &LanguageCase, repository: &Path) -> String {
    let fixture = repository.join(case.fixture);
    let mut source = std::fs::read_to_string(fixture).unwrap();
    match case.language {
        "rust" => source.push_str(
            "\nasync fn receiver_contract(client: Client, handle: Handle) {\n    client.fetch().await.unwrap();\n    handle.await.len();\n}\n",
        ),
        "javascript" => source.push_str(RECEIVER_EDGE_SOURCE),
        "ruby" => source.push_str("\ndef receiverContract(service)\n  service&.run()\nend\n"),
        "dart" => source.push_str(
            "\nvoid receiverContract(Service service) {\n  service..run();\n  service?..run();\n}\n",
        ),
        "r" => source.push_str("\nreceiverContract <- function(object) object@slot\n"),
        "html" => {
            source = format!("<!-- 🥝 -->\n{EMBEDDED_NON_MEMBER_HOST_CONTENT}\n{source}")
        }
        "vue" => {
            source = format!("<!-- 🥝 -->\n{EMBEDDED_NON_MEMBER_HOST_CONTENT}\n{source}")
        }
        "vbnet" => source = source.replace("With _svc", "With System.Service"),
        "bash" => source.push_str("\n# Run on interrupt.\ntrap cleanup EXIT\n"),
        "powershell" => {
            source = source.replace(
                "Get-Process | Select-Object -First 1",
                "dotnet restore .\n        Get-Process | Select-Object -First 1",
            )
        }
        "fsharp" => source.push_str(
            "\nlet receiverContract x = match x with | Some value -> log value\n",
        ),
        "razor" => source.push_str("\n<td>@item.Price.ToString(\"C\")</td>\n"),
        _ => {}
    }
    source
}

fn expected_spans(
    source: &str,
    snippet: &str,
    target: &str,
    expected_occurrences: usize,
    label: &str,
    failures: &mut Vec<String>,
) -> Vec<(u32, u32, u32, u32, u32, u32)> {
    let Some(target_offset) = snippet.find(target) else {
        failures.push(format!(
            "{label}: target {target:?} is absent from snippet {snippet:?}"
        ));
        return Vec::new();
    };
    let matches: Vec<_> = source.match_indices(snippet).collect();
    if matches.len() != expected_occurrences {
        failures.push(format!(
            "{label}: expected {expected_occurrences} occurrence(s) of {snippet:?}, found {}",
            matches.len()
        ));
    }
    matches
        .into_iter()
        .map(|(snippet_start, _)| {
            let start = snippet_start + target_offset;
            let end = start + target.len();
            let (start_line, start_column) = line_column(source, start);
            let (end_line, end_column) = line_column(source, end);
            (
                start as u32,
                end as u32,
                start_line,
                start_column,
                end_line,
                end_column,
            )
        })
        .collect()
}

fn line_column(source: &str, byte: usize) -> (u32, u32) {
    let prefix = &source[..byte];
    let line = prefix.bytes().filter(|byte| *byte == b'\n').count() as u32 + 1;
    let column = prefix.rsplit('\n').next().unwrap().len() as u32;
    (line, column)
}

fn receiver_from_metadata(
    metadata: Option<&std::collections::HashMap<String, Value>>,
) -> Option<String> {
    metadata?.get("receiver")?.as_str().map(str::to_owned)
}

fn qualifier_from_metadata(
    metadata: Option<&std::collections::HashMap<String, Value>>,
) -> Option<String> {
    metadata?
        .get("receiver_qualifier")?
        .as_str()
        .map(str::to_owned)
}

fn canonical_facts(
    identifiers: &[julie_extractors::Identifier],
    failures: &mut Vec<String>,
) -> Vec<ReceiverFact> {
    let mut facts = Vec::new();
    for identifier in identifiers {
        let receiver = receiver_from_metadata(identifier.metadata.as_ref());
        let qualifier = qualifier_from_metadata(identifier.metadata.as_ref());
        if let Some(receiver) = receiver {
            let kind = serde_json::to_value(&identifier.kind)
                .unwrap()
                .as_str()
                .unwrap()
                .to_string();
            facts.push(ReceiverFact {
                path: identifier.file_path.clone(),
                name: identifier.name.clone(),
                kind,
                start_line: identifier.start_line,
                start_column: identifier.start_column,
                end_line: identifier.end_line,
                end_column: identifier.end_column,
                start_byte: identifier.start_byte,
                end_byte: identifier.end_byte,
                receiver,
                qualifier,
            });
        } else if qualifier.is_some() {
            failures.push(format!(
                "{}:{} has a receiver qualifier without a named receiver",
                identifier.file_path, identifier.name
            ));
        }
    }
    facts.sort();
    facts
}

fn artifact_identifiers(db: &Path) -> Vec<ArtifactIdentifier> {
    let connection = Connection::open(db).unwrap();
    let mut statement = connection
        .prepare(
            "SELECT i.path, i.name, i.kind, i.start_line, i.start_column,
                    i.end_line, i.end_column, i.start_byte, i.end_byte,
                    i.metadata_json, r.start_line, r.start_column, r.end_line,
                    r.end_column, r.start_byte, r.end_byte, r.is_exact, i.language
             FROM identifiers i
             JOIN reference_sites r ON r.reference_site_id = i.reference_site_id
             ORDER BY i.path, i.start_byte, i.end_byte, i.name, i.kind",
        )
        .unwrap();
    statement
        .query_map([], |row| {
            let metadata_json: Option<String> = row.get(9)?;
            let metadata = metadata_json
                .as_deref()
                .map(serde_json::from_str::<Value>)
                .transpose()
                .unwrap_or_default();
            let receiver_value = metadata.as_ref().and_then(|value| value.get("receiver"));
            let qualifier_value = metadata
                .as_ref()
                .and_then(|value| value.get("receiver_qualifier"));
            Ok(ArtifactIdentifier {
                path: row.get(0)?,
                language: row.get(17)?,
                name: row.get(1)?,
                kind: row.get(2)?,
                start_line: row.get::<_, i64>(3)? as u32,
                start_column: row.get::<_, i64>(4)? as u32,
                end_line: row.get::<_, i64>(5)? as u32,
                end_column: row.get::<_, i64>(6)? as u32,
                start_byte: row.get::<_, i64>(7)? as u32,
                end_byte: row.get::<_, i64>(8)? as u32,
                receiver: receiver_value.and_then(Value::as_str).map(str::to_owned),
                qualifier: qualifier_value.and_then(Value::as_str).map(str::to_owned),
                receiver_key_present: receiver_value.is_some(),
                qualifier_key_present: qualifier_value.is_some(),
                reference_site_span: (
                    row.get(10)?,
                    row.get(11)?,
                    row.get(12)?,
                    row.get(13)?,
                    row.get(14)?,
                    row.get(15)?,
                ),
                reference_site_exact: row.get::<_, i64>(16)? == 1,
            })
        })
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

fn fact_from_artifact(identifier: &ArtifactIdentifier) -> Option<ReceiverFact> {
    Some(ReceiverFact {
        path: identifier.path.clone(),
        name: identifier.name.clone(),
        kind: identifier.kind.clone(),
        start_line: identifier.start_line,
        start_column: identifier.start_column,
        end_line: identifier.end_line,
        end_column: identifier.end_column,
        start_byte: identifier.start_byte,
        end_byte: identifier.end_byte,
        receiver: identifier.receiver.clone()?,
        qualifier: identifier.qualifier.clone(),
    })
}

fn fact_counts(facts: &[ReceiverFact]) -> BTreeMap<ReceiverFact, usize> {
    let mut counts = BTreeMap::new();
    for fact in facts {
        *counts.entry(fact.clone()).or_default() += 1;
    }
    counts
}

fn multiset_difference(
    expected: &BTreeMap<ReceiverFact, usize>,
    actual: &BTreeMap<ReceiverFact, usize>,
) -> Vec<(ReceiverFact, usize)> {
    expected
        .iter()
        .filter_map(|(fact, expected_count)| {
            let actual_count = actual.get(fact).copied().unwrap_or_default();
            (*expected_count > actual_count).then(|| (fact.clone(), *expected_count - actual_count))
        })
        .take(20)
        .collect()
}

fn check_expected_receivers(
    case: &LanguageCase,
    source: &str,
    relative_path: &str,
    identifiers: &[julie_extractors::Identifier],
    artifact: &[ArtifactIdentifier],
    failures: &mut Vec<String>,
) {
    for (snippet, target, expected_receiver, expected_qualifier, count) in case.expected {
        let label = format!("{} {target}", case.language);
        for (start, end, start_line, start_column, end_line, end_column) in
            expected_spans(source, snippet, target, *count, &label, failures)
        {
            let site_label = format!("{label} at bytes {start}..{end}");
            let at_site: Vec<_> = identifiers
                .iter()
                .filter(|identifier| {
                    identifier.name == *target
                        && identifier.start_byte == start
                        && identifier.end_byte == end
                })
                .collect();
            if at_site.is_empty() {
                failures.push(format!("{site_label}: no canonical identifier"));
            } else if at_site.len() != 1 {
                failures.push(format!(
                    "{site_label}: expected one canonical identifier, found {} with kinds {:?}",
                    at_site.len(),
                    at_site
                        .iter()
                        .map(|identifier| &identifier.kind)
                        .collect::<Vec<_>>()
                ));
            }
            for identifier in at_site {
                if identifier.start_line != start_line
                    || identifier.start_column != start_column
                    || identifier.end_line != end_line
                    || identifier.end_column != end_column
                {
                    failures.push(format!(
                        "{site_label}: canonical span was {}:{}-{}:{} bytes {}..{}, expected {}:{}-{}:{} bytes {}..{}",
                        identifier.start_line,
                        identifier.start_column,
                        identifier.end_line,
                        identifier.end_column,
                        identifier.start_byte,
                        identifier.end_byte,
                        start_line,
                        start_column,
                        end_line,
                        end_column,
                        start,
                        end
                    ));
                }
                if identifier.language != case.identifier_language.unwrap_or(case.language) {
                    failures.push(format!(
                        "{site_label}: identifier language was {}, expected {}",
                        identifier.language,
                        case.identifier_language.unwrap_or(case.language)
                    ));
                }
                if receiver_from_metadata(identifier.metadata.as_ref()).as_deref()
                    != Some(*expected_receiver)
                {
                    failures.push(format!(
                        "{site_label}: canonical receiver was {:?}, expected {:?} (kind={:?}, language={}, metadata={:?})",
                        receiver_from_metadata(identifier.metadata.as_ref()),
                        expected_receiver,
                        identifier.kind,
                        identifier.language,
                        identifier.metadata
                    ));
                }
                if qualifier_from_metadata(identifier.metadata.as_ref()).as_deref()
                    != *expected_qualifier
                {
                    failures.push(format!(
                        "{site_label}: canonical qualifier was {:?}, expected {:?}",
                        qualifier_from_metadata(identifier.metadata.as_ref()),
                        expected_qualifier
                    ));
                }
            }

            let artifact_at_site: Vec<_> = artifact
                .iter()
                .filter(|identifier| {
                    identifier.path == relative_path
                        && identifier.name == *target
                        && identifier.start_byte == start
                        && identifier.end_byte == end
                })
                .collect();
            if artifact_at_site.is_empty() {
                failures.push(format!("{site_label}: no artifact identifier"));
            } else if artifact_at_site.len() != 1 {
                failures.push(format!(
                    "{site_label}: expected one artifact identifier, found {} with kinds {:?}",
                    artifact_at_site.len(),
                    artifact_at_site
                        .iter()
                        .map(|identifier| &identifier.kind)
                        .collect::<Vec<_>>()
                ));
            }
            for identifier in artifact_at_site {
                if identifier.language != case.language {
                    failures.push(format!(
                        "{site_label}: artifact language was {}, expected {}",
                        identifier.language, case.language
                    ));
                }
                if identifier.start_line != start_line
                    || identifier.start_column != start_column
                    || identifier.end_line != end_line
                    || identifier.end_column != end_column
                {
                    failures.push(format!(
                        "{site_label}: artifact span was {}:{}-{}:{} bytes {}..{}, expected {}:{}-{}:{} bytes {}..{}",
                        identifier.start_line,
                        identifier.start_column,
                        identifier.end_line,
                        identifier.end_column,
                        identifier.start_byte,
                        identifier.end_byte,
                        start_line,
                        start_column,
                        end_line,
                        end_column,
                        start,
                        end
                    ));
                }
                if identifier.receiver.as_deref() != Some(*expected_receiver)
                    || identifier.qualifier.as_deref() != *expected_qualifier
                {
                    failures.push(format!(
                        "{site_label}: artifact receiver/qualifier was {:?}/{:?}, expected {:?}/{:?}",
                        identifier.receiver,
                        identifier.qualifier,
                        expected_receiver,
                        expected_qualifier
                    ));
                }
            }
        }
    }
}

fn check_expected_absences(
    language: &str,
    source: &str,
    identifiers: &[julie_extractors::Identifier],
    artifact: &[ArtifactIdentifier],
    relative_path: &str,
    expected: &[ExpectedAbsence],
    failures: &mut Vec<String>,
) {
    for (snippet, target, count) in expected {
        let label = format!("{language} must not infer {target}");
        for (start, end, _, _, _, _) in
            expected_spans(source, snippet, target, *count, &label, failures)
        {
            let canonical_at_site: Vec<_> = identifiers
                .iter()
                .filter(|identifier| {
                    identifier.name == *target
                        && identifier.start_byte == start
                        && identifier.end_byte == end
                })
                .collect();
            if canonical_at_site.is_empty() {
                failures.push(format!("{label}: no canonical identifier at byte {start}"));
            } else if canonical_at_site.len() != 1 {
                failures.push(format!(
                    "{label}: expected one canonical identifier at byte {start}, found {} with kinds {:?}",
                    canonical_at_site.len(),
                    canonical_at_site
                        .iter()
                        .map(|identifier| &identifier.kind)
                        .collect::<Vec<_>>()
                ));
            }
            if canonical_at_site.iter().any(|identifier| {
                receiver_from_metadata(identifier.metadata.as_ref()).is_some()
                    || qualifier_from_metadata(identifier.metadata.as_ref()).is_some()
            }) {
                failures.push(format!("{label}: canonical metadata has a receiver"));
            }
            let artifact_at_site: Vec<_> = artifact
                .iter()
                .filter(|identifier| {
                    identifier.path == relative_path
                        && identifier.name == *target
                        && identifier.start_byte == start
                        && identifier.end_byte == end
                })
                .collect();
            if artifact_at_site.is_empty() {
                failures.push(format!("{label}: no artifact identifier at byte {start}"));
            } else if artifact_at_site.len() != 1 {
                failures.push(format!(
                    "{label}: expected one artifact identifier at byte {start}, found {} with kinds {:?}",
                    artifact_at_site.len(),
                    artifact_at_site
                        .iter()
                        .map(|identifier| &identifier.kind)
                        .collect::<Vec<_>>()
                ));
            }
            if artifact_at_site.iter().any(|identifier| {
                identifier.receiver_key_present || identifier.qualifier_key_present
            }) {
                failures.push(format!("{label}: artifact metadata contains receiver keys"));
            }
        }
    }
}

fn check_unextracted_identifiers(
    language: &str,
    source: &str,
    identifiers: &[julie_extractors::Identifier],
    artifact: &[ArtifactIdentifier],
    relative_path: &str,
    expected: &[ExpectedAbsence],
    failures: &mut Vec<String>,
) {
    for (snippet, target, count) in expected {
        let label = format!("{language} must not extract {target}");
        for (start, end, _, _, _, _) in
            expected_spans(source, snippet, target, *count, &label, failures)
        {
            if identifiers.iter().any(|identifier| {
                identifier.name == *target
                    && identifier.start_byte == start
                    && identifier.end_byte == end
            }) {
                failures.push(format!(
                    "{label}: canonical identifier exists at byte {start}"
                ));
            }
            if artifact.iter().any(|identifier| {
                identifier.path == relative_path
                    && identifier.name == *target
                    && identifier.start_byte == start
                    && identifier.end_byte == end
            }) {
                failures.push(format!(
                    "{label}: artifact identifier exists at byte {start}"
                ));
            }
        }
    }
}

#[test]
fn receiver_facts_cover_every_registered_language_and_survive_artifact_mapping() {
    let repository = repository_root();
    let scratch = TempDir::new().unwrap();
    let root = scratch.path().join("repo");
    std::fs::create_dir(&root).unwrap();
    let mut failures = Vec::new();

    let registered: BTreeSet<_> = supported_languages().into_iter().collect();
    let covered: BTreeSet<_> = LANGUAGE_CASES.iter().map(|case| case.language).collect();
    if registered != covered || covered.len() != LANGUAGE_CASES.len() {
        failures.push(format!(
            "receiver applicability matrix differs from live registry; missing={:?}, unregistered={:?}, duplicate_rows={}",
            registered.difference(&covered).collect::<Vec<_>>(),
            covered.difference(&registered).collect::<Vec<_>>(),
            LANGUAGE_CASES.len() - covered.len()
        ));
    }

    let mut canonical = Vec::new();
    let mut source_cases = Vec::new();
    let mut written_paths = BTreeSet::new();
    let all_cases = LANGUAGE_CASES
        .iter()
        .map(|case| (case, true))
        .chain(SUPPLEMENTAL_CASES.iter().map(|case| (case, false)));
    for (case, add_contract_source) in all_cases {
        if case.expected.is_empty() && case.not_applicable.is_none() {
            failures.push(format!(
                "{} has neither a receiver expectation nor a non-applicability reason",
                case.language
            ));
        }
        if !case.expected.is_empty() && case.not_applicable.is_some() {
            failures.push(format!(
                "{} is both applicable and marked not applicable",
                case.language
            ));
        }

        let source = if add_contract_source {
            prepare_source(case, &repository)
        } else {
            std::fs::read_to_string(repository.join(case.fixture)).unwrap()
        };
        let fixture_path = case.fixture.strip_prefix("fixtures/extraction/").unwrap();
        let relative_path = format!("cases/{fixture_path}");
        if !written_paths.insert(relative_path.clone()) {
            failures.push(format!(
                "fixture output path is duplicated: {relative_path}"
            ));
        }
        let file_path = root.join(&relative_path);
        std::fs::create_dir_all(file_path.parent().unwrap()).unwrap();
        std::fs::write(&file_path, &source).unwrap();

        match extract_canonical_for_language_at(
            case.language,
            &relative_path,
            &source,
            &root,
            ExtractionLevel::Full,
        ) {
            Ok(results) => {
                if let Some(reason) = case.not_applicable {
                    let receiver_rows = results
                        .identifiers
                        .iter()
                        .filter(|identifier| {
                            receiver_from_metadata(identifier.metadata.as_ref()).is_some()
                        })
                        .count();
                    if receiver_rows != 0 {
                        failures.push(format!(
                            "{} is marked not applicable ({reason}) but emitted {receiver_rows} receiver row(s)",
                            case.language
                        ));
                    }
                }
                canonical.extend(canonical_facts(&results.identifiers, &mut failures));
                source_cases.push((case, relative_path, source, results.identifiers));
            }
            Err(error) => failures.push(format!(
                "{} canonical extraction failed: {error:#}",
                case.language
            )),
        }
    }

    let edge_relative_path = "cases/javascript/receiver_edge_cases.js";
    if !written_paths.insert(edge_relative_path.to_string()) {
        failures.push(format!(
            "fixture output path is duplicated: {edge_relative_path}"
        ));
    }
    let edge_path = root.join(edge_relative_path);
    std::fs::write(&edge_path, RECEIVER_EDGE_SOURCE).unwrap();
    match extract_canonical_for_language_at(
        "javascript",
        edge_relative_path,
        RECEIVER_EDGE_SOURCE,
        &root,
        ExtractionLevel::Full,
    ) {
        Ok(results) => {
            canonical.extend(canonical_facts(&results.identifiers, &mut failures));
            source_cases.push((
                &RECEIVER_EDGE_CASE,
                edge_relative_path.to_string(),
                RECEIVER_EDGE_SOURCE.to_string(),
                results.identifiers,
            ));
        }
        Err(error) => failures.push(format!("javascript edge extraction failed: {error:#}")),
    }

    let db = scratch.path().join("artifact.sqlite");
    let output = Command::new(env!("CARGO_BIN_EXE_julie-extract"))
        .args(["scan", "--root"])
        .arg(&root)
        .arg("--db")
        .arg(&db)
        .args(["--level", "full", "--jobs", "1", "--json"])
        .output()
        .unwrap();
    if !output.status.success() {
        failures.push(format!(
            "CLI scan failed:\n{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ));
    } else {
        let artifact = artifact_identifiers(&db);
        let artifact_facts: Vec<_> = artifact.iter().filter_map(fact_from_artifact).collect();
        let mut sorted_canonical = canonical;
        sorted_canonical.sort();
        let mut sorted_artifact = artifact_facts;
        sorted_artifact.sort();
        if sorted_canonical != sorted_artifact {
            let canonical_counts = fact_counts(&sorted_canonical);
            let artifact_counts = fact_counts(&sorted_artifact);
            let missing = multiset_difference(&canonical_counts, &artifact_counts);
            let extra = multiset_difference(&artifact_counts, &canonical_counts);
            failures.push(format!(
                "canonical and SQLite receiver fact multiplicities differ: canonical={}, SQLite={}, missing={missing:?}, extra={extra:?}",
                sorted_canonical.len(),
                sorted_artifact.len()
            ));
        }

        for identifier in &artifact {
            if identifier.receiver.is_some() {
                let expected_reference_site = (
                    Some(identifier.start_line as i64),
                    Some(identifier.start_column as i64),
                    Some(identifier.end_line as i64),
                    Some(identifier.end_column as i64),
                    Some(identifier.start_byte as i64),
                    Some(identifier.end_byte as i64),
                );
                if !identifier.reference_site_exact
                    || identifier.reference_site_span != expected_reference_site
                {
                    failures.push(format!(
                        "{}:{} SQLite identifier and reference-site spans differ",
                        identifier.path, identifier.name
                    ));
                }
            }
        }

        for (case, relative_path, source, identifiers) in &source_cases {
            check_expected_receivers(
                case,
                source,
                relative_path,
                identifiers,
                &artifact,
                &mut failures,
            );
            check_expected_absences(
                case.language,
                source,
                identifiers,
                &artifact,
                relative_path,
                case.absences,
                &mut failures,
            );
            if case.language == "javascript" {
                check_unextracted_identifiers(
                    case.language,
                    source,
                    identifiers,
                    &artifact,
                    relative_path,
                    RECEIVER_EDGE_UNEXTRACTED,
                    &mut failures,
                );
            }
            if matches!(case.language, "html" | "vue") {
                check_unextracted_identifiers(
                    case.language,
                    source,
                    identifiers,
                    &artifact,
                    relative_path,
                    &[(EMBEDDED_NON_MEMBER_HOST_CONTENT, "send", 1)],
                    &mut failures,
                );
            }
            if case.not_applicable.is_some() {
                let artifact_receivers = artifact
                    .iter()
                    .filter(|identifier| {
                        identifier.path == *relative_path && identifier.receiver.is_some()
                    })
                    .count();
                if artifact_receivers != 0 {
                    failures.push(format!(
                        "{} artifact emitted {artifact_receivers} receiver row(s) despite non-applicability",
                        case.language
                    ));
                }
            }
        }
    }

    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
