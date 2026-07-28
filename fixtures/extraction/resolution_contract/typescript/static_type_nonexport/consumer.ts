// Cross-file reference to a non-exported type name — must stay unresolved.
export function remoteUse(): number {
  return Hidden.create();
}
