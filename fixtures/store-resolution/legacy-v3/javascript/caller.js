import { jsUnique as importedUnique } from "./defs.js";
import { absent as absentModule } from "./missing-module.js";

export function caller() {
  importedUnique();
  jsCollision();
  missingJs();
  absentModule();
  const object = { value: 1 };
  return object.value;
}
