import { beforeEach, describe, test } from "vitest";

describe("tsx roles", () => {
  beforeEach(() => {});
  test("renders a TSX test case", () => {
    const view = <output>ready</output>;
    return view;
  });
});

function testNamedButOrdinary(): JSX.Element {
  return <span>ordinary</span>;
}

const ordinary = {
  test(_name: string, callback: () => JSX.Element): JSX.Element {
    return callback();
  },
};

ordinary.test("ordinary member call", () => <span>not a test</span>);
