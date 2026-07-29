import type { Fixture } from "./fixture";

export function run(): number {
  // Type-only import must not authorize a runtime static call.
  return Fixture.create();
}
