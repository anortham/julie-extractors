<template>
  <output>ready</output>
</template>

<script setup lang="ts">
describe("vue embedded roles", () => {
  beforeEach(() => {});
  test("renders a Vue test case", () => {});
});

function testNamedButOrdinary(): void {}

const ordinary = {
  test(_name: string, callback: () => void): void {
    callback();
  },
};

ordinary.test("ordinary member call", () => {});
</script>
