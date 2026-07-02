// Vue extractor helper utilities and regex patterns
//
// Contains shared regex patterns and helper functions used across Vue extraction modules

use regex::Regex;
use std::sync::LazyLock;

// Static regex patterns compiled once for performance.
//
// Invariant for every `expect` below: the pattern is a compile-time regex
// literal validated by the test suite, so `Regex::new` cannot fail at runtime.
pub(super) static TEMPLATE_START_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^<template(\s+[^>]*)?>").expect("TEMPLATE_START_RE literal regex must compile")
});

pub(super) static SCRIPT_START_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^<script(\s+[^>]*)?>").expect("SCRIPT_START_RE literal regex must compile")
});

pub(super) static STYLE_START_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^<style(\s+[^>]*)?>").expect("STYLE_START_RE literal regex must compile")
});

pub(super) static SECTION_END_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^</(template|script|style)>").expect("SECTION_END_RE literal regex must compile")
});

pub(super) static LANG_ATTR_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"lang=["']?([^"'\s>]+)"#).expect("LANG_ATTR_RE literal regex must compile")
});

pub(super) static DATA_FUNCTION_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^\s*data\s*\(\s*\)\s*\{").expect("DATA_FUNCTION_RE literal regex must compile")
});

pub(super) static METHODS_OBJECT_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^\s*methods\s*:\s*\{").expect("METHODS_OBJECT_RE literal regex must compile")
});

pub(super) static COMPUTED_OBJECT_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^\s*computed\s*:\s*\{").expect("COMPUTED_OBJECT_RE literal regex must compile")
});

pub(super) static PROPS_OBJECT_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^\s*props\s*:\s*\{").expect("PROPS_OBJECT_RE literal regex must compile")
});

pub(super) static FUNCTION_DEF_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^\s*([a-zA-Z_$][a-zA-Z0-9_$]*)\s*\([^)]*\)\s*\{")
        .expect("FUNCTION_DEF_RE literal regex must compile")
});
