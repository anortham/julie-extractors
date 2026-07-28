// Non-exported class: file-local visibility. Cross-file static-type receivers
// must refuse to bind `Hidden.create()` even though the simple name is unique.
class Hidden {
  static create(): number {
    return 1;
  }
}

export function localUse(): number {
  return Hidden.create();
}
