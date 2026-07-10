describe("jsx roles", () => {
  beforeEach(() => {});
  test("renders a JSX test case", () => {
    const view = <output>ready</output>;
    return view;
  });
});

function testNamedButOrdinary() {
  return <span>ordinary</span>;
}

const ordinary = {
  test(_name, callback) {
    return callback();
  },
};

ordinary.test("ordinary member call", () => <span>not a test</span>);
