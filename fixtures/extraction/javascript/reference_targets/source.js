function target() {
  return 1;
}

function parameter_shadow(target) {
  return target();
}

function local_shadow() {
  const target = 2;
  return target();
}

function forward_caller() {
  return forward_target();
}

function forward_target() {
  return 3;
}

function owner_one() {
  function private_target() {
    return 4;
  }
}

function owner_two() {
  return private_target();
}

function sibling_owner() {
  function nested_target() {
    return 5;
  }
  function nested_caller() {
    return nested_target();
  }
  return nested_caller();
}

class MethodHolder {
  method_target() {
    return 6;
  }
  method_caller() {
    return method_target();
  }
}

class Receiver {
  render() {
    return 7;
  }
  arrow_caller() {
    const nested = () => this.render();
    return nested();
  }
  function_caller() {
    function nested() {
      return this.render();
    }
    return nested();
  }
}

const namedExpression = function privateName() {
  return privateName();
};

function namedExpressionOutside() {
  return privateName();
}
