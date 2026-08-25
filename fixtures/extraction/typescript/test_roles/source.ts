import { beforeEach, describe, it } from "vitest";
import { params, suite, test } from "@testdeck/mocha";

describe("typescript roles", () => {
  beforeEach(() => {});
  it("extracts a TypeScript test case", () => {});
});

@suite
class CartSuite {
  @test
  addsALineItem(): void {}

  @test.only
  keepsAFocusedCase(): void {}

  @params({ quantity: 1 })
  @params({ quantity: 2 })
  scalesQuantity(input: { quantity: number }): number {
    return input.quantity;
  }

  buildCart(): number {
    return 0;
  }
}

function testNamedButOrdinary(): void {}

const ordinary = {
  test(_name: string, callback: () => void): void {
    callback();
  },
};

ordinary.test("ordinary member call", () => {});
