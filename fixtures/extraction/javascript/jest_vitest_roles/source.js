import {
  afterAll,
  afterEach,
  beforeAll,
  beforeEach,
  bench,
  describe,
  it,
  test,
} from "vitest";

function total(items) {
  return items.length;
}

describe("cart totals", () => {
  beforeAll(() => {});
  beforeEach(() => {});
  afterEach(() => {});
  afterAll(() => {});

  it("adds a line item", () => {});
  it.only("keeps a focused case", () => {});
  it.skip("keeps a disabled case", () => {});
  test.todo("keeps a planned case");
  test.failing("keeps a known-failing case", () => {});
  xit("keeps the legacy disabled spelling", () => {});
  fit("keeps the legacy focused spelling", () => {});
  xtest("keeps the legacy disabled alias", () => {});

  test.each([1, 2])("doubles %i", (value) => total([value]));
  describe.each([1, 2])("scales %i", (value) => total([value]));

  bench("measures the total", () => total([1]));
});

xdescribe("legacy disabled group", () => {});
fdescribe("legacy focused group", () => {});

export function testTotalsHelper(items) {
  return total(items);
}

const reporter = {
  test(name, run) {
    return run(name);
  },
};

reporter.test("member call that only looks like a case", () => {});
