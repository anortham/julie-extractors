// Same-file static-type receiver: no import path, so tier2 cannot bind.
export class Fixture {
  static create() {
    return 1;
  }
}

export function run() {
  return Fixture.create();
}
