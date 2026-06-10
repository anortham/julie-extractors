class Worker:
    def __init__(self, id: int) -> None:
        self.id = id

    def run(self) -> int:
        return helper(self.id)

    @staticmethod
    def default_id() -> int:
        return 0


def helper(value: int) -> int:
    """Increment a worker id."""
    return value + 1
