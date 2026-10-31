struct Holder { value: i32 }
fn consume(_: i32) {}
fn mutate(value: &mut i32) { *value += 1; }
fn borrow(_: &i32) {}
fn writes(initial: i32, mut holder: Holder, mut argument: i32, records: Option<i32>) {
 let initialized = initial;
 let mut plain = 0; plain = initial;
 let mut compound = 0; compound += initial;
 holder.value = initial;
 let mut items = vec![0]; let index = 0; items[index] = initial;
 let offset = 0; items[offset + index] = initial;
 consume(argument); borrow(&argument); mutate(&mut argument);
 match records { Some(captured) => consume(captured), None => {} }
}
fn update(mut ptr: &mut i32, other: &mut i32, holder: &mut Holder) {
 ptr = other;
 *ptr = 1;
 *ptr += 1;
 (*holder).value = 2;
}
fn guard(value: Option<i32>, mut limit: i32) -> i32 {
 match value {
  Some(n) if { limit += 1; n > limit } => n,
  _ => 0,
 }
}
