// No import of Hidden (it is not exported). Must not resolve.
export function crossFile(): number {
  // @ts-expect-error intentional unresolved external-looking name for the gate
  return Hidden.create();
}
