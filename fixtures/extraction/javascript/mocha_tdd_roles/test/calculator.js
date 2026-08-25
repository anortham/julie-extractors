suite("calculator tdd", () => {
  suiteSetup(() => {});
  setup(() => {});
  teardown(() => {});
  suiteTeardown(() => {});

  test("adds two operands", () => {});
  test.only("keeps a focused case", () => {});
  test.skip("keeps a disabled case", () => {});
});

describe("calculator bdd", () => {
  before(() => {});
  beforeEach(() => {});
  afterEach(() => {});
  after(() => {});

  context("with two operands", () => {
    specify("multiplies", () => {});
    it("divides", () => {});
  });

  xcontext("disabled operands", () => {});
});

function buildCalculator() {
  return { add: (left, right) => left + right };
}

const harness = {
  suite(name, run) {
    return run(name);
  },
};

harness.suite("member call that only looks like a suite", buildCalculator);
