import test, { after, afterEach, before, beforeEach, describe, it } from "node:test";

function Panel({ label }) {
  return <section>{label}</section>;
}

describe("panel", () => {
  before(() => {});
  beforeEach(() => {});
  afterEach(() => {});
  after(() => {});

  it("renders a label", () => {
    return <Panel label="ready" />;
  });
});

test("panel lifecycle", async (t) => {
  await t.test("mounts", () => <Panel label="mounted" />);
  await t.test("unmounts", () => <Panel label="unmounted" />);
  await t.beforeEach(() => {});
  t.diagnostic("not a role");
});

function renderPanel(label) {
  return <Panel label={label} />;
}

const runner = {
  it(name, run) {
    return run(name);
  },
};

runner.it("member call that only looks like a case", renderPanel);
