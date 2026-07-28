// Cross-file static-type references. No import: the receiver is the type name.
export function run(): number {
  const made = Fixture.create();
  return made + Limits.max();
}
