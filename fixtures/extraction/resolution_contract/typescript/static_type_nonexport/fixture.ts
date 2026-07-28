// Not exported: same-file static access may bind; cross-file must refuse.
class Hidden {
  static create(): number {
    return 1;
  }
}

export function sameFile(): number {
  return Hidden.create();
}
