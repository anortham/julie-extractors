import * as inner from "./inner";
import * as outer from "./outer";

export function readValue() {
    return outer.get(inner.get("value"));
}
