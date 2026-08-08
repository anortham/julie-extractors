import { unique as importedUnique } from "./defs";
import { absent as absentModule } from "./missing-module";

export function caller(): number {
  importedUnique();
  collision();
  missingCall();
  absentModule();
  const object = { value: 1 };
  return object.value;
}
