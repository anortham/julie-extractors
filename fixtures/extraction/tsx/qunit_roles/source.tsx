import QUnit from "qunit";

interface Assert {
  equal(actual: unknown, expected: unknown): void;
}

function Badge({ label }: { label: string }): JSX.Element {
  return <span>{label}</span>;
}

QUnit.module("badge", (hooks: { beforeEach(run: () => void): void }) => {
  hooks.beforeEach(() => {});

  QUnit.test("renders a label", (assert: Assert) => {
    assert.equal(<Badge label="ready" />, <Badge label="ready" />);
  });

  QUnit.only("keeps a focused case", (assert: Assert) => {});
  QUnit.skip("keeps a disabled case", (assert: Assert) => {});
  QUnit.todo("keeps a planned case", (assert: Assert) => {});
});

QUnit.module.only("badge focused group", () => {
  QUnit.test("still a case", (assert: Assert) => {});
});

function renderBadge(label: string): JSX.Element {
  return <Badge label={label} />;
}

const runner = {
  module(name: string, run: () => void): void {
    run();
  },
};

runner.module("member call that only looks like a group", renderBadge);
