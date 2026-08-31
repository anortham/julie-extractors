/// Reads one value without moving it.
///
/// ```rust
/// let value = 42;
/// assert_eq!(read_flag(&value), 42);
/// ```
///
/// ```rust,no_run
/// let value = 7;
/// let _ = read_flag(&value);
/// ```
pub fn read_flag(value: &i32) -> i32 {
    unsafe {
        core::ptr::read_volatile(value)
    }
}
