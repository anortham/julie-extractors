import { expect, test } from "@playwright/test";

interface Page {
  goto(url: string): Promise<void>;
  title(): Promise<string>;
}

test.describe("checkout", () => {
  test.beforeAll(async () => {});
  test.beforeEach(async () => {});
  test.afterEach(async () => {});
  test.afterAll(async () => {});

  test("loads the cart", async ({ page }: { page: Page }) => {
    await page.goto("/cart");
    expect(await page.title()).toBe("Cart");
  });

  test.only("keeps a focused case", async () => {});
  test.skip("keeps a disabled case", async () => {});

  test("records a step", async ({ page }: { page: Page }) => {
    await test.step("open the cart", async () => {
      await page.goto("/cart");
    });
  });
});

test.describe.serial("checkout in order", () => {
  test("pays", async () => {});
});

test.describe.parallel("checkout at once", () => {
  test("ships", async () => {});
});

function buildCheckout(): Page {
  return {
    goto: async () => {},
    title: async () => "Cart",
  };
}

const runner = {
  describe(name: string, run: () => void): void {
    run();
  },
};

runner.describe("member call that only looks like a group", buildCheckout);
