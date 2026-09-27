def module_target():
    return 1

def parameter_shadow(module_target):
    return module_target()

def local_shadow():
    module_target = 2
    return module_target()

def owner_one():
    def private_target():
        return 3

def owner_two():
    return private_target()

def forward_caller():
    return forward_target()

def forward_target():
    return 4

def recursive(value):
    return recursive(value - 1) if value else 0

def sibling_owner():
    def nested_target():
        return 5

    def nested_caller():
        return nested_target()

    return nested_caller()

class MethodHolder:
    def module_target(self):
        return 6

def module_caller():
    return module_target()
