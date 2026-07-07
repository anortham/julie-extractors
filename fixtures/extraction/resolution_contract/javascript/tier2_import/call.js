// Tier 2 (cross-file import) reference half for JavaScript.
import { produceWidget } from "./def.js";

export function consume() {
  return produceWidget();
}
