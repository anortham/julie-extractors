// Static-type receiver for TypeScript: the receiver names a type directly
// rather than a variable whose type must be inferred, so no `type_facts` row
// participates. `Fixture.create()` and `Limits.max` both resolve at
// tier3_static_type from another file because the types are exported
// (visibility Public) and the members are static.
export class Fixture {
  static create(): number {
    return 1;
  }
}

export class Limits {
  static max(): number {
    return 10;
  }
}
