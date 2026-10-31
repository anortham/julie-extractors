global_plain = 0
global_plain = 1

class Holder:
    def __init__(self):
        self.value = 0

def consume(value):
    return value

def writes(initial, holder, argument, records):
    initialized = initial
    plain = 0
    plain = initial
    compound = 0
    compound += initial
    holder.value = initial
    consume(argument)
    match records:
        case captured:
            consume(captured)
