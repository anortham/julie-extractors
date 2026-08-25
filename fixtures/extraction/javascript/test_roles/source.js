import { beforeEach, describe, test } from "vitest";

describe("javascript roles", () => {
  beforeEach(() => {});
  test("extracts a JavaScript test case", () => {});
});

function testNamedButOrdinary() {}

const ordinary = {
  test(_name, callback) {
    callback();
  },
};

ordinary.test("ordinary member call", () => {});
