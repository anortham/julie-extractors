// Tier 2 (cross-file import) reference half: a named import brings the local
// binding `produceWidget` into scope; the call is resolved through the import
// contract at tier 2.
import { produceWidget } from "./def";

export function consume(): number {
  return produceWidget();
}
