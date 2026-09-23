//! A tokenizer declared as a file-as-struct.
const Tokenizer = @This();

buffer: []const u8,
index: usize,

pub fn next(self: *Tokenizer) ?u8 {
    if (self.index >= self.buffer.len) return null;
    const byte = self.peek();
    self.index += 1;
    return byte;
}

fn peek(self: *const Tokenizer) u8 {
    return self.buffer[self.index];
}
