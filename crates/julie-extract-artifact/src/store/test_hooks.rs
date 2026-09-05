const CRASH_BOUNDARY_ENV: &str = "JULIE_EXTRACT_STORE_TEST_CRASH_AT";
const CRASH_MARKER_ENV: &str = "JULIE_EXTRACT_STORE_TEST_CRASH_MARKER";

#[doc(hidden)]
pub fn crash_if(boundary: &str) {
    if std::env::var(CRASH_BOUNDARY_ENV).as_deref() == Ok(boundary) {
        if let Some(marker_path) = std::env::var_os(CRASH_MARKER_ENV) {
            use std::io::Write;

            let mut marker = std::fs::File::create(marker_path)
                .expect("failed to create requested crash marker");
            marker
                .write_all(boundary.as_bytes())
                .expect("failed to write requested crash marker");
            marker
                .sync_all()
                .expect("failed to flush requested crash marker");
        }
        std::process::abort();
    }
}
