use std::path::Path;
use std::sync::Arc;

use julie_extract_artifact::store::{
    CoordinatorError, LeaseHolder, StoreCoordinator, StoreLayout,
};

use super::import::{ImportClock, ImportPidLiveness};
use super::report::{StoreOperation, StoreReport, StoreRequestState, StoreRequestedLevel};

pub(crate) fn quote_identifier(ident: &str) -> String {
    format!("\"{}\"", ident.replace('"', "\"\""))
}

pub(crate) fn valid_blake3_hash(hash: &str) -> bool {
    hash.strip_prefix("blake3:").is_some_and(|value| {
        value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
    })
}

pub(crate) fn valid_root_relative_path(root: &Path, path: &str) -> bool {
    !path.is_empty()
        && path.len() <= super::args::MAX_STORE_PATH_BYTES
        && !path.starts_with('/')
        && !path.contains(['\\', ':', '\0'])
        && !path
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
        && root.join(path).starts_with(root)
}

pub(crate) fn base_report(
    operation: StoreOperation,
    request_id: &str,
    family_id: &str,
    view_id: &str,
    root: impl AsRef<Path>,
    requested_level: StoreRequestedLevel,
    idempotency_key: &str,
    state: StoreRequestState,
) -> StoreReport {
    StoreReport::new(request_id, family_id, view_id, state)
        .with_operation(operation)
        .with_idempotency_key(idempotency_key)
        .with_root(root.as_ref().to_string_lossy())
        .with_requested_level(requested_level)
}

pub(crate) fn cli_lease_holder_id() -> String {
    format!("cli-{}", std::process::id())
}

pub(crate) fn cli_lease_holder() -> LeaseHolder {
    LeaseHolder::new(
        cli_lease_holder_id(),
        env!("CARGO_PKG_VERSION"),
        std::process::id(),
    )
}

pub(crate) fn open_cli_coordinator(
    layout: &StoreLayout,
) -> Result<StoreCoordinator, CoordinatorError> {
    StoreCoordinator::open_with_runtime(
        layout,
        cli_lease_holder(),
        Arc::new(ImportClock),
        Arc::new(ImportPidLiveness),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_quote_identifier() {
        assert_eq!(quote_identifier("table"), "\"table\"");
        assert_eq!(quote_identifier("my \"table\""), "\"my \"\"table\"\"\"");
    }

    #[test]
    fn test_valid_blake3_hash() {
        assert!(valid_blake3_hash(
            "blake3:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
        ));
        assert!(!valid_blake3_hash("sha256:0123456789abcdef"));
        assert!(!valid_blake3_hash("blake3:0123456789abcdef"));
        assert!(!valid_blake3_hash(
            "blake3:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdeg"
        ));
    }

    #[test]
    fn test_valid_root_relative_path() {
        let root = Path::new("/workspace");
        assert!(valid_root_relative_path(root, "src/lib.rs"));
        assert!(valid_root_relative_path(root, "file.txt"));
        assert!(!valid_root_relative_path(root, ""));
        assert!(!valid_root_relative_path(root, "/src/lib.rs"));
        assert!(!valid_root_relative_path(root, "src/../lib.rs"));
        assert!(!valid_root_relative_path(root, "src/./lib.rs"));
        assert!(!valid_root_relative_path(root, "src//lib.rs"));
        assert!(!valid_root_relative_path(root, "src\\lib.rs"));
    }
}
