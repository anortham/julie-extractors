import { beforeEach, describe, test } from "vitest";

describe("typescript roles", () => {
  beforeEach(() => {});
  test("extracts a TypeScript test case", () => {});
});

function testNamedButOrdinary(): void {}

const ordinary = {
  test(_name: string, callback: () => void): void {
    callback();
  },
};

ordinary.test("ordinary member call", () => {});
