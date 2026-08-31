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
///
/// ```should_panic
/// panic!("read_flag rejects this");
/// ```
///
/// ```text
/// not executable
/// ```
pub fn read_flag(value: &i32) -> i32 {
    unsafe {
        core::ptr::read_volatile(value)
    }
}

/** Doubles a value.

```
assert_eq!(double_flag(2), 4);
```
*/
pub fn double_flag(value: i32) -> i32 {
    value * 2
}

/**
 * Halves a value.
 *
 * ```rust,no_run
 * let _ = halve_flag(8);
 * ```
 */
pub fn halve_flag(value: i32) -> i32 {
    value / 2
}
