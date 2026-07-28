// Static-type receiver for JavaScript: exported class with a static method.
// `Fixture.create()` resolves at tier3_static_type from another file because
// the class is Public (JS default) and the method carries isStatic / `static`.
export class Fixture {
  static create() {
    return 1;
  }
}
