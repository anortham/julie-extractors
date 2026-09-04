//! Language Support - Shared tree-sitter language configuration
//!
//! This module provides the public language metadata API.

pub use crate::language_spec::{
    DATA_ONLY_CAPABILITIES, DocCommentStyle, FULL_CAPABILITIES, LanguageCapabilities, LanguageSpec,
    NO_PENDING_CAPABILITIES, NO_RELATIONSHIP_CAPABILITIES, PENDING_NO_TYPES_CAPABILITIES,
    detect_language_for_path, detect_language_for_source, detect_language_from_extension,
    get_tree_sitter_language, language_spec, language_specs, supported_extensions,
    supported_languages,
};

pub(crate) use crate::language_spec::detect_language_with_tree;
