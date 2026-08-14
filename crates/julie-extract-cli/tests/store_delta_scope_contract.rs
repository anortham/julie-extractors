use julie_extract_artifact::store::{
    ManifestEntry, ManifestPublishResult, ManifestStore, create_store_schema,
};
use rusqlite::{Connection, params};

pub use julie_extract_cli::{resolution, resolution_session};

#[path = "../src/store/delta_scope.rs"]
mod delta_scope;

use delta_scope::{
    StoreDeltaScopeDecision, StoreDeltaScopeFullReason, StoreDeltaScopeRequest,
    build_store_delta_scope,
};
use resolution_session::SemanticVersionId;

const NOW: &str = "2026-08-11T18:00:00Z";

#[test]
fn store_scope_matches_legacy_name_file_row_and_decision_expansion() {
    let mut connection = scope_store();
    let old_target = insert_version(&connection, "src/util.ts", "old");
    let new_target = insert_version(&connection, "src/util.ts", "new");
    let importer = insert_version(&connection, "src/user.ts", "user");
    let receiver = insert_version(&connection, "src/receiver.ts", "receiver");
    let tier_four = insert_version(&connection, "src/other.ts", "other");
    let padding = insert_version(&connection, "src/padding.ts", "padding");
    insert_symbol(&connection, old_target, "old-foo", "Foo", "function", None);
    insert_symbol(&connection, new_target, "new-foo", "Foo", "function", None);
    insert_symbol(
        &connection,
        importer,
        "import-foo",
        "Foo",
        "import",
        Some(r#"{"alias":"Bar","source":"./util"}"#),
    );
    insert_identifier(&connection, importer, "identifier-bar", "Bar", None);
    insert_symbol(
        &connection,
        receiver,
        "receiver-symbol",
        "widget",
        "variable",
        None,
    );
    connection
        .execute(
            "INSERT INTO type_facts
             (version_id,type_fact_id,symbol_id,language,resolved_type,is_inferred)
             VALUES (?1,'receiver-type','receiver-symbol','typescript','Foo',0)",
            [receiver],
        )
        .unwrap();
    insert_pending(
        &connection,
        receiver,
        "pending-widget",
        "receiver-symbol",
        "member",
        Some("widget"),
    );
    insert_identifier(&connection, tier_four, "identifier-foo", "Foo", None);
    insert_identifier(
        &connection,
        padding,
        "identifier-padding-a",
        "PaddingA",
        None,
    );
    insert_identifier(
        &connection,
        padding,
        "identifier-padding-b",
        "PaddingB",
        None,
    );
    insert_identifier(
        &connection,
        padding,
        "identifier-padding-c",
        "PaddingC",
        None,
    );
    insert_identifier(
        &connection,
        padding,
        "identifier-padding-d",
        "PaddingD",
        None,
    );

    let first_entries = [
        entry(&connection, old_target),
        entry(&connection, importer),
        entry(&connection, receiver),
        entry(&connection, tier_four),
        entry(&connection, padding),
    ];
    let first = publish(&mut connection, None, first_entries, "request-first");
    let first_generation = i64::try_from(first.generation).unwrap();
    bind_exact(&connection, &first.manifest_hash, first_generation, 11, 7);
    let second_entries = [
        entry(&connection, new_target),
        entry(&connection, importer),
        entry(&connection, receiver),
        entry(&connection, tier_four),
        entry(&connection, padding),
    ];
    let second = publish(
        &mut connection,
        Some(first_generation),
        second_entries,
        "request-second",
    );

    let decision = build_store_delta_scope(
        &connection,
        StoreDeltaScopeRequest {
            view_id: "view-a",
            manifest_generation: i64::try_from(second.generation).unwrap(),
            manifest_hash: &second.manifest_hash,
            resolver_output_epoch: 7,
            incremental_enabled: true,
        },
    )
    .unwrap();
    let StoreDeltaScopeDecision::Scoped(worklists) = decision else {
        panic!("legacy-equivalent fixture must remain scoped");
    };

    assert_eq!(worklists.recheck_names, ["Bar", "Foo", "widget"]);
    assert_eq!(
        worklists.recheck_versions,
        [SemanticVersionId::Store(new_target)]
    );
    assert_eq!(
        worklists.selected_versions,
        [
            SemanticVersionId::Store(new_target),
            SemanticVersionId::Store(importer),
            SemanticVersionId::Store(receiver),
            SemanticVersionId::Store(tier_four),
        ]
    );
    assert_eq!(
        worklists.changed_versions,
        [SemanticVersionId::Store(new_target)]
    );
    assert!(!worklists.effective_full);
}

#[test]
fn disabled_incremental_scope_returns_named_full_fallback() {
    let connection = scope_store();
    let decision = build_store_delta_scope(
        &connection,
        StoreDeltaScopeRequest {
            view_id: "view-a",
            manifest_generation: 1,
            manifest_hash: "manifest-a",
            resolver_output_epoch: 7,
            incremental_enabled: false,
        },
    )
    .unwrap();

    let StoreDeltaScopeDecision::Full { worklists, reason } = decision else {
        panic!("disabled incremental scope must select full resolution");
    };
    assert_eq!(reason, StoreDeltaScopeFullReason::EnvironmentDisabled);
    assert_eq!(reason.as_str(), "incremental_resolution_disabled");
    assert!(worklists.effective_full);
    assert!(matches!(
        worklists.scope,
        resolution_session::ResolutionWorklistScope::Corpus
    ));
}

#[test]
fn added_and_deleted_paths_recheck_module_importers() {
    let (added, added_target, added_importer) = structural_scope(false, true);
    let StoreDeltaScopeDecision::Scoped(added_worklists) = added else {
        panic!("added-path fixture must remain scoped");
    };
    assert_eq!(
        added_worklists.recheck_versions,
        [
            SemanticVersionId::Store(added_target),
            SemanticVersionId::Store(added_importer),
        ]
    );

    let (deleted, _deleted_target, deleted_importer) = structural_scope(true, false);
    let StoreDeltaScopeDecision::Scoped(deleted_worklists) = deleted else {
        panic!("deleted-path fixture must remain scoped");
    };
    assert_eq!(
        deleted_worklists.recheck_versions,
        [SemanticVersionId::Store(deleted_importer)]
    );
    assert!(deleted_worklists.changed_versions.is_empty());
}

#[test]
fn journal_count_hash_chain_epoch_and_predecessor_failures_have_distinct_full_reasons() {
    let (count_store, count_manifest) = replacement_scope();
    count_store
        .execute_batch("DROP TRIGGER trg_resolution_scope_batch_immutable_update")
        .unwrap();
    count_store
        .execute(
            "UPDATE resolution_scope_batches SET change_count=change_count+1
             WHERE transition_id=(SELECT MAX(transition_id) FROM resolution_scope_batches)",
            [],
        )
        .unwrap();
    assert_eq!(
        fallback_reason(&count_store, &count_manifest, 7),
        StoreDeltaScopeFullReason::JournalCountMismatch
    );

    let (hash_store, hash_manifest) = replacement_scope();
    hash_store
        .execute_batch("DROP TRIGGER trg_resolution_scope_journal_immutable_update")
        .unwrap();
    hash_store
        .execute(
            "UPDATE resolution_scope_journal SET touched_names_json='[\"Corrupt\"]'
             WHERE transition_id=(SELECT MAX(transition_id) FROM resolution_scope_batches)",
            [],
        )
        .unwrap();
    assert_eq!(
        fallback_reason(&hash_store, &hash_manifest, 7),
        StoreDeltaScopeFullReason::JournalHashMismatch
    );

    let (chain_store, chain_manifest) = multi_transition_scope();
    chain_store
        .execute_batch("DROP TRIGGER trg_resolution_scope_batch_immutable_update")
        .unwrap();
    chain_store
        .execute(
            "UPDATE resolution_scope_batches SET previous_transition_id=1
             WHERE transition_id=(SELECT MAX(transition_id) FROM resolution_scope_batches)",
            [],
        )
        .unwrap();
    assert_eq!(
        fallback_reason(&chain_store, &chain_manifest, 7),
        StoreDeltaScopeFullReason::JournalChainBroken
    );

    let (epoch_store, epoch_manifest) = replacement_scope();
    assert_eq!(
        fallback_reason(&epoch_store, &epoch_manifest, 8),
        StoreDeltaScopeFullReason::ResolverEpochMismatch
    );

    let (predecessor_store, predecessor_manifest) = replacement_scope();
    predecessor_store
        .execute(
            "INSERT INTO resolution_bases
             (base_id,manifest_hash,resolver_output_epoch,state,relative_path,identifier_count,
              pending_count,file_bytes,file_sha256,request_id,created_at,updated_at)
             VALUES ('base-b','unrelated',7,'ready','bases/base-b.db',0,0,1,'sha256:b',
                     'request-other',?1,?1)",
            [NOW],
        )
        .unwrap();
    predecessor_store
        .execute_batch("DROP TRIGGER trg_resolution_scope_batch_immutable_update")
        .unwrap();
    predecessor_store
        .execute(
            "UPDATE resolution_scope_batches SET base_id='base-b'
             WHERE transition_id=(SELECT MAX(transition_id) FROM resolution_scope_batches)",
            [],
        )
        .unwrap();
    assert_eq!(
        fallback_reason(&predecessor_store, &predecessor_manifest, 7),
        StoreDeltaScopeFullReason::JournalPredecessorMismatch
    );
}

#[test]
fn crossover_promotes_multi_file_scope() {
    let (multi_store, multi_manifest) = two_file_replacement_scope();
    let multi = scope_decision(&multi_store, &multi_manifest, 7);
    let StoreDeltaScopeDecision::Full { reason, .. } = &multi else {
        panic!("dense multi-file scope must cross over to full resolution");
    };
    assert_eq!(*reason, StoreDeltaScopeFullReason::Crossover);
    assert!(multi.worklists().effective_full);
}

#[test]
fn one_changed_file_with_broad_name_collisions_crosses_over() {
    let (connection, manifest) = broad_name_collision_scope();
    let total_identifiers = connection
        .query_row(
            "SELECT COUNT(*)
             FROM identifiers AS identifier
             JOIN manifest_entries AS entry ON entry.version_id=identifier.version_id
             WHERE entry.view_id='view-a' AND entry.generation=?1
               AND entry.status IN ('indexed','failed_preserved')",
            [i64::try_from(manifest.generation).unwrap()],
            |row| row.get::<_, i64>(0),
        )
        .unwrap();
    let changed_paths = connection
        .query_row(
            "SELECT change_count FROM resolution_scope_batches
             WHERE transition_id=(SELECT MAX(transition_id) FROM resolution_scope_batches)",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap();
    assert_eq!(changed_paths, 1);
    assert_eq!(total_identifiers, 100);

    let decision = scope_decision(&connection, &manifest, 7);
    let StoreDeltaScopeDecision::Full { reason, worklists } = decision else {
        panic!(
            "one changed path must cross over when its name arm selects all {total_identifiers} identifier rows"
        );
    };
    assert_eq!(reason, StoreDeltaScopeFullReason::Crossover);
    assert!(worklists.effective_full);
}

#[test]
fn empty_identifier_crossover_counts_deleted_logical_files() {
    let (connection, manifest) = deleted_paths_crossover_scope();
    let decision = scope_decision(&connection, &manifest, 7);
    let StoreDeltaScopeDecision::Full { reason, .. } = decision else {
        panic!("three deleted logical files must cross over against one current file");
    };
    assert_eq!(reason, StoreDeltaScopeFullReason::Crossover);
}

fn replacement_scope() -> (Connection, ManifestPublishResult) {
    let mut connection = scope_store();
    let old = insert_version(&connection, "src/lib.ts", "old");
    let new = insert_version(&connection, "src/lib.ts", "new");
    insert_symbol(&connection, old, "old", "Old", "function", None);
    insert_symbol(&connection, new, "new", "New", "function", None);
    let first_entries = [entry(&connection, old)];
    let first = publish(&mut connection, None, first_entries, "request-first");
    let first_generation = i64::try_from(first.generation).unwrap();
    bind_exact(&connection, &first.manifest_hash, first_generation, 11, 7);
    let second_entries = [entry(&connection, new)];
    let second = publish(
        &mut connection,
        Some(first_generation),
        second_entries,
        "request-second",
    );
    (connection, second)
}

fn multi_transition_scope() -> (Connection, ManifestPublishResult) {
    let (mut connection, second) = replacement_scope();
    let newest = insert_version(&connection, "src/lib.ts", "newest");
    insert_symbol(&connection, newest, "newest", "Newest", "function", None);
    let third_entries = [entry(&connection, newest)];
    let third = publish(
        &mut connection,
        Some(i64::try_from(second.generation).unwrap()),
        third_entries,
        "request-third",
    );
    (connection, third)
}

fn two_file_replacement_scope() -> (Connection, ManifestPublishResult) {
    let mut connection = scope_store();
    let old_a = insert_version(&connection, "src/a.ts", "old-a");
    let old_b = insert_version(&connection, "src/b.ts", "old-b");
    let new_a = insert_version(&connection, "src/a.ts", "new-a");
    let new_b = insert_version(&connection, "src/b.ts", "new-b");
    insert_symbol(&connection, old_a, "old-a", "A", "function", None);
    insert_symbol(&connection, old_b, "old-b", "B", "function", None);
    insert_symbol(&connection, new_a, "new-a", "A", "function", None);
    insert_symbol(&connection, new_b, "new-b", "B", "function", None);
    let first_entries = [entry(&connection, old_a), entry(&connection, old_b)];
    let first = publish(&mut connection, None, first_entries, "request-first");
    let first_generation = i64::try_from(first.generation).unwrap();
    bind_exact(&connection, &first.manifest_hash, first_generation, 11, 7);
    let second_entries = [entry(&connection, new_a), entry(&connection, new_b)];
    let second = publish(
        &mut connection,
        Some(first_generation),
        second_entries,
        "request-second",
    );
    (connection, second)
}

fn broad_name_collision_scope() -> (Connection, ManifestPublishResult) {
    let mut connection = scope_store();
    let old_target = insert_version(&connection, "src/target.ts", "old-target");
    let new_target = insert_version(&connection, "src/target.ts", "new-target");
    insert_symbol(
        &connection,
        old_target,
        "old-shared",
        "Shared",
        "function",
        None,
    );
    insert_symbol(
        &connection,
        new_target,
        "new-shared",
        "Shared",
        "function",
        None,
    );
    for index in 0..10 {
        insert_identifier(
            &connection,
            old_target,
            &format!("old-target-{index}"),
            "Shared",
            None,
        );
        insert_identifier(
            &connection,
            new_target,
            &format!("new-target-{index}"),
            "Shared",
            None,
        );
    }

    let mut old_entries = vec![entry(&connection, old_target)];
    let mut new_entries = vec![entry(&connection, new_target)];
    for file in 0..9 {
        let version = insert_version(
            &connection,
            &format!("src/collision-{file}.ts"),
            &format!("collision-{file}"),
        );
        insert_symbol(
            &connection,
            version,
            &format!("collision-symbol-{file}"),
            &format!("Collision{file}"),
            "function",
            None,
        );
        for identifier in 0..10 {
            insert_identifier(
                &connection,
                version,
                &format!("collision-{file}-{identifier}"),
                "Shared",
                None,
            );
        }
        old_entries.push(entry(&connection, version));
        new_entries.push(entry(&connection, version));
    }

    let first = publish(&mut connection, None, old_entries, "request-first");
    let first_generation = i64::try_from(first.generation).unwrap();
    bind_exact(&connection, &first.manifest_hash, first_generation, 11, 7);
    let second = publish(
        &mut connection,
        Some(first_generation),
        new_entries,
        "request-second",
    );
    (connection, second)
}

fn deleted_paths_crossover_scope() -> (Connection, ManifestPublishResult) {
    let mut connection = scope_store();
    let retained = insert_version(&connection, "src/retained.ts", "retained");
    let deleted_a = insert_version(&connection, "src/deleted-a.ts", "deleted-a");
    let deleted_b = insert_version(&connection, "src/deleted-b.ts", "deleted-b");
    let deleted_c = insert_version(&connection, "src/deleted-c.ts", "deleted-c");
    let first_entries = [
        entry(&connection, retained),
        entry(&connection, deleted_a),
        entry(&connection, deleted_b),
        entry(&connection, deleted_c),
    ];
    let first = publish(&mut connection, None, first_entries, "request-first");
    let first_generation = i64::try_from(first.generation).unwrap();
    bind_exact(&connection, &first.manifest_hash, first_generation, 11, 7);
    let second_entries = [entry(&connection, retained)];
    let second = publish(
        &mut connection,
        Some(first_generation),
        second_entries,
        "request-second",
    );
    (connection, second)
}

fn fallback_reason(
    connection: &Connection,
    manifest: &ManifestPublishResult,
    resolver_output_epoch: i64,
) -> StoreDeltaScopeFullReason {
    let decision = scope_decision(connection, manifest, resolver_output_epoch);
    let StoreDeltaScopeDecision::Full { reason, .. } = decision else {
        panic!("corrupt scope must select full resolution");
    };
    reason
}

fn scope_decision(
    connection: &Connection,
    manifest: &ManifestPublishResult,
    resolver_output_epoch: i64,
) -> StoreDeltaScopeDecision {
    build_store_delta_scope(
        connection,
        StoreDeltaScopeRequest {
            view_id: "view-a",
            manifest_generation: i64::try_from(manifest.generation).unwrap(),
            manifest_hash: &manifest.manifest_hash,
            resolver_output_epoch,
            incremental_enabled: true,
        },
    )
    .unwrap()
}

fn structural_scope(
    target_in_first: bool,
    target_in_second: bool,
) -> (StoreDeltaScopeDecision, i64, i64) {
    let mut connection = scope_store();
    let target = insert_version(&connection, "src/util.ts", "util");
    let importer = insert_version(&connection, "src/user.ts", "user");
    let padding = (0..4)
        .map(|index| {
            insert_version(
                &connection,
                &format!("src/padding-{index}.ts"),
                &format!("padding-{index}"),
            )
        })
        .collect::<Vec<_>>();
    insert_symbol(&connection, target, "utility", "Utility", "function", None);
    insert_symbol(
        &connection,
        importer,
        "import-utility",
        "Utility",
        "import",
        Some(r#"{"source":"./util"}"#),
    );
    let mut first_entries = vec![entry(&connection, importer)];
    first_entries.extend(padding.iter().map(|version| entry(&connection, *version)));
    if target_in_first {
        first_entries.push(entry(&connection, target));
    }
    let first = publish(&mut connection, None, first_entries, "request-first");
    let first_generation = i64::try_from(first.generation).unwrap();
    bind_exact(&connection, &first.manifest_hash, first_generation, 11, 7);
    let mut second_entries = vec![entry(&connection, importer)];
    second_entries.extend(padding.iter().map(|version| entry(&connection, *version)));
    if target_in_second {
        second_entries.push(entry(&connection, target));
    }
    let second = publish(
        &mut connection,
        Some(first_generation),
        second_entries,
        "request-second",
    );
    let decision = build_store_delta_scope(
        &connection,
        StoreDeltaScopeRequest {
            view_id: "view-a",
            manifest_generation: i64::try_from(second.generation).unwrap(),
            manifest_hash: &second.manifest_hash,
            resolver_output_epoch: 7,
            incremental_enabled: true,
        },
    )
    .unwrap();
    (decision, target, importer)
}

fn scope_store() -> Connection {
    let connection = Connection::open_in_memory().unwrap();
    create_store_schema(&connection).unwrap();
    connection
}

fn insert_version(connection: &Connection, path: &str, hash: &str) -> i64 {
    connection
        .execute(
            "INSERT INTO file_versions
             (path,content_hash,extraction_epoch,language,content_bytes,complete_l1,complete_l2)
             VALUES (?1,?2,1,'typescript',1,1,1)",
            params![path, hash],
        )
        .unwrap();
    connection.last_insert_rowid()
}

fn insert_symbol(
    connection: &Connection,
    version_id: i64,
    symbol_id: &str,
    name: &str,
    kind: &str,
    metadata_json: Option<&str>,
) {
    connection
        .execute(
            "INSERT INTO symbols
             (version_id,symbol_id,path,language,name,kind,start_line,start_column,end_line,
              end_column,start_byte,end_byte,metadata_json)
             SELECT ?1,?2,path,language,?3,?4,1,0,1,1,0,1,?5
             FROM file_versions WHERE version_id=?1",
            params![version_id, symbol_id, name, kind, metadata_json],
        )
        .unwrap();
}

fn insert_identifier(
    connection: &Connection,
    version_id: i64,
    identifier_id: &str,
    name: &str,
    receiver: Option<&str>,
) {
    let reference_site_id = format!("site-{identifier_id}");
    connection
        .execute(
            "INSERT INTO reference_sites
             (version_id,reference_site_id,path,language,start_line,start_column,end_line,
              end_column,start_byte,end_byte,is_exact,provenance,level)
             SELECT ?1,?2,path,language,1,0,1,1,0,1,1,'target_token',1
             FROM file_versions WHERE version_id=?1",
            params![version_id, reference_site_id],
        )
        .unwrap();
    let metadata = receiver.map(|value| serde_json::json!({"receiver": value}).to_string());
    connection
        .execute(
            "INSERT INTO identifiers
             (version_id,identifier_id,reference_site_id,path,language,name,kind,start_line,
              start_column,end_line,end_column,start_byte,end_byte,confidence,metadata_json)
             SELECT ?1,?2,?3,path,language,?4,'identifier',1,0,1,1,0,1,1.0,?5
             FROM file_versions WHERE version_id=?1",
            params![version_id, identifier_id, reference_site_id, name, metadata],
        )
        .unwrap();
}

fn insert_pending(
    connection: &Connection,
    version_id: i64,
    pending_id: &str,
    from_symbol_id: &str,
    terminal_name: &str,
    receiver: Option<&str>,
) {
    let reference_site_id = format!("site-{pending_id}");
    connection
        .execute(
            "INSERT INTO reference_sites
             (version_id,reference_site_id,path,language,start_line,start_column,end_line,
              end_column,start_byte,end_byte,is_exact,provenance,level)
             SELECT ?1,?2,path,language,1,0,1,1,0,1,1,'target_token',1
             FROM file_versions WHERE version_id=?1",
            params![version_id, reference_site_id],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO pending_relationships
             (version_id,pending_relationship_id,reference_site_id,from_symbol_id,path,kind,
              target_display_name,target_terminal_name,target_receiver,target_namespace_json,
              start_line,start_column,end_line,end_column,start_byte,end_byte,confidence)
             SELECT ?1,?2,?3,?4,path,'reference',?5,?5,?6,'[]',1,0,1,1,0,1,1.0
             FROM file_versions WHERE version_id=?1",
            params![
                version_id,
                pending_id,
                reference_site_id,
                from_symbol_id,
                terminal_name,
                receiver
            ],
        )
        .unwrap();
}

fn entry(connection: &Connection, version_id: i64) -> ManifestEntry {
    connection
        .query_row(
            "SELECT path,language,content_hash FROM file_versions WHERE version_id=?1",
            [version_id],
            |row| {
                Ok(ManifestEntry::indexed(
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    version_id,
                    row.get::<_, String>(2)?,
                    NOW,
                ))
            },
        )
        .unwrap()
}

fn publish(
    connection: &mut Connection,
    expected_generation: Option<i64>,
    entries: impl IntoIterator<Item = ManifestEntry>,
    request_id: &str,
) -> ManifestPublishResult {
    let mut store = ManifestStore::new(connection);
    store.ensure_view("view-a", "/repo").unwrap();
    store
        .publish(
            "view-a",
            expected_generation.map(|value| value as u64),
            entries,
            request_id,
        )
        .unwrap()
}

fn bind_exact(
    connection: &Connection,
    manifest_hash: &str,
    generation: i64,
    delta_generation: i64,
    resolver_output_epoch: i64,
) {
    connection
        .execute(
            "INSERT INTO resolution_bases
             (base_id,manifest_hash,resolver_output_epoch,state,relative_path,identifier_count,
              pending_count,file_bytes,file_sha256,request_id,created_at,updated_at)
             VALUES ('base-a',?1,?2,'ready','bases/base-a.db',0,0,1,'sha256:a',
                     'request-resolve',?3,?3)",
            params![manifest_hash, resolver_output_epoch, NOW],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO resolution_deltas
             (view_id,delta_generation,base_id,manifest_generation,manifest_hash,
              resolver_output_epoch,identifier_replacements,pending_replacements,
              pending_tombstones,exact_gap_rows,exact_gap_files,exact_gap_json,request_id,created_at)
             VALUES ('view-a',?1,'base-a',?2,?3,?4,0,0,0,0,0,'[]','request-resolve',?5)",
            params![
                delta_generation,
                generation,
                manifest_hash,
                resolver_output_epoch,
                NOW
            ],
        )
        .unwrap();
    connection
        .execute(
            "UPDATE views
             SET resolution_state='exact',resolution_base_id='base-a',
                 resolution_delta_generation=?1,resolution_exact_at=?2
             WHERE view_id='view-a'",
            params![delta_generation, generation],
        )
        .unwrap();
}
