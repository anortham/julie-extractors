import { missing as alias } from "./does-not-exist";

export function consume(): number {
  return alias();
}
