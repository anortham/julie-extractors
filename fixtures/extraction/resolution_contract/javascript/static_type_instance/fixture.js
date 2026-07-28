// Instance method only: `Fixture.run()` via the type name must not bind —
// instance members are not statically reachable through a type receiver.
export class Fixture {
  run() {
    return 1;
  }
}
