const CRASH_BOUNDARY_ENV: &str = "JULIE_EXTRACT_STORE_TEST_CRASH_AT";

#[doc(hidden)]
pub fn crash_if(boundary: &str) {
    if std::env::var(CRASH_BOUNDARY_ENV).as_deref() == Ok(boundary) {
        std::process::abort();
    }
}
