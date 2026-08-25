//! Direction gate for the shared call-style test vocabularies.
//!
//! `test_call_role` reads a lifecycle hook's direction out of the callee name,
//! so a word added to any vocabulary silently picks a half. These tests declare
//! the intended half for every word and fail on any word that is not declared,
//! which turns a silent misclassification into a build failure.
//!
//! A hook that wraps a test case on both sides (Quick's `aroundEach`) records
//! the setup half, because a wrapping hook always runs its setup part first.

use crate::base::TestRole;
use crate::test_calls::{SHARED_TEST_CALL_VOCABS, TestCallCategory, test_call_role};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

const VOCAB_DECLARATION_MARKER: &str = ": TestCallVocab = TestCallVocab {";

const SETUP_HALF_WORDS: &[&str] = &[
    "before",
    "beforeAll",
    "beforeEach",
    "beforeEachTest",
    "beforeGroup",
    "beforeSuite",
    "beforeTest",
    "before_each",
    "setUp",
    "setUpAll",
    "setup",
    "lazy_setup",
    "justBeforeEach",
    "aroundEach",
    "BeforeAll",
    "BeforeEach",
    "BeforeSuite",
    "JustBeforeEach",
];

const TEARDOWN_HALF_WORDS: &[&str] = &[
    "after",
    "afterAll",
    "afterEach",
    "afterEachTest",
    "afterGroup",
    "afterSuite",
    "afterTest",
    "after_each",
    "tearDown",
    "tearDownAll",
    "teardown",
    "lazy_teardown",
    "AfterAll",
    "AfterEach",
    "AfterSuite",
    "JustAfterEach",
    "DeferCleanup",
];

fn production_sources(root: &Path) -> Vec<PathBuf> {
    let mut sources = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(directory) = stack.pop() {
        let entries = fs::read_dir(&directory)
            .unwrap_or_else(|err| panic!("failed to read {}: {err}", directory.display()));
        for entry in entries {
            let path = entry
                .expect("source directory entry should be readable")
                .path();
            if path.is_dir() {
                if path.file_name().and_then(|name| name.to_str()) != Some("tests") {
                    stack.push(path);
                }
            } else if path.extension().and_then(|extension| extension.to_str()) == Some("rs") {
                sources.push(path);
            }
        }
    }
    sources.sort();
    sources
}

fn declared_vocab_constants() -> BTreeSet<String> {
    let source_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut declared = BTreeSet::new();

    for path in production_sources(&source_root) {
        let source = fs::read_to_string(&path)
            .unwrap_or_else(|err| panic!("failed to read {}: {err}", path.display()));
        for line in source.lines() {
            let Some(head) = line
                .split_once(VOCAB_DECLARATION_MARKER)
                .map(|(head, _)| head)
            else {
                continue;
            };
            let Some(name) = head.split_whitespace().last() else {
                continue;
            };
            declared.insert(name.to_string());
        }
    }
    declared
}

fn declared_direction(word: &str) -> Option<TestRole> {
    if SETUP_HALF_WORDS.contains(&word) {
        return Some(TestRole::FixtureSetup);
    }
    if TEARDOWN_HALF_WORDS.contains(&word) {
        return Some(TestRole::FixtureTeardown);
    }
    None
}

#[test]
fn every_shared_lifecycle_word_publishes_its_declared_direction() {
    let mut checked = 0usize;

    for (vocab_name, vocab) in SHARED_TEST_CALL_VOCABS {
        for word in vocab.lifecycle {
            let expected = declared_direction(word).unwrap_or_else(|| {
                panic!(
                    "{vocab_name} lifecycle word `{word}` declares no direction; \
                     add it to SETUP_HALF_WORDS or TEARDOWN_HALF_WORDS"
                )
            });
            assert_eq!(
                test_call_role(word, TestCallCategory::Lifecycle),
                expected,
                "{vocab_name} lifecycle word `{word}` publishes the wrong half"
            );
            checked += 1;
        }
    }

    assert!(
        checked >= 45,
        "gate should walk every registered lifecycle word, walked only {checked}"
    );
}

#[test]
fn every_declared_lifecycle_word_belongs_to_a_registered_vocabulary() {
    let registered: BTreeSet<&str> = SHARED_TEST_CALL_VOCABS
        .iter()
        .flat_map(|(_, vocab)| vocab.lifecycle.iter().copied())
        .collect();

    let orphans: Vec<&str> = SETUP_HALF_WORDS
        .iter()
        .chain(TEARDOWN_HALF_WORDS)
        .copied()
        .filter(|word| !registered.contains(word))
        .collect();

    assert!(
        orphans.is_empty(),
        "declared words no vocabulary uses: {orphans:?}"
    );
}

#[test]
fn no_lifecycle_word_declares_both_halves() {
    let both: Vec<&&str> = SETUP_HALF_WORDS
        .iter()
        .filter(|word| TEARDOWN_HALF_WORDS.contains(word))
        .collect();

    assert!(both.is_empty(), "words declared on both halves: {both:?}");
}

#[test]
fn every_shared_case_and_container_word_publishes_its_category_role() {
    for (vocab_name, vocab) in SHARED_TEST_CALL_VOCABS {
        for word in vocab.test {
            assert_eq!(
                test_call_role(word, TestCallCategory::Test),
                TestRole::TestCase,
                "{vocab_name} test word `{word}`"
            );
        }
        for word in vocab.container {
            assert_eq!(
                test_call_role(word, TestCallCategory::Container),
                TestRole::TestContainer,
                "{vocab_name} container word `{word}`"
            );
        }
    }
}

#[test]
fn every_language_with_a_shared_vocabulary_is_registered() {
    let registered: BTreeSet<&str> = SHARED_TEST_CALL_VOCABS
        .iter()
        .map(|(name, _)| *name)
        .collect();
    let declared = declared_vocab_constants();

    let missing: Vec<&String> = declared
        .iter()
        .filter(|const_name| {
            !registered
                .iter()
                .any(|entry| entry.ends_with(const_name.as_str()))
        })
        .collect();

    assert!(
        !declared.is_empty(),
        "source scan should find the vocabulary declarations; the marker probably drifted"
    );
    assert!(
        missing.is_empty(),
        "vocabularies missing from SHARED_TEST_CALL_VOCABS: {missing:?}"
    );
}
