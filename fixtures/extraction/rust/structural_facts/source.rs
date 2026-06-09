pub fn read_flag(value: &i32) -> i32 {
    unsafe {
        core::ptr::read_volatile(value)
    }
}
