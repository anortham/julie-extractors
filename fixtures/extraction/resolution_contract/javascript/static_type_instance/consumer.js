import { Fixture } from "./fixture.js";

export function run() {
  // Instance method via type name must not resolve.
  return Fixture.create();
}
