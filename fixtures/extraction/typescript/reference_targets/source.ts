const namedExpression = function privateName(): number {
  return privateName();
};

function namedExpressionOutside(): number {
  return privateName();
}
