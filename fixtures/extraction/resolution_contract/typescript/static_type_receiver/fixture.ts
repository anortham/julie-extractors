// Same-file static-type receiver: no import path, so tier2 cannot bind.
// Fixture.create() resolves via tier3_static_type.
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

export function run(): number {
  const made = Fixture.create();
  return made + Limits.max();
}
