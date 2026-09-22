"""Modern Python declarations, kinds, docstrings, and references."""

import asyncio
import enum
from typing import Literal, Protocol

from app.models import Order, User
from app.shapes import Circle, Point

# A plain comment does not document a constant.
MAX_RETRIES = 3

#: Default timeout in seconds.
#: Applies to every request.
DEFAULT_TIMEOUT: float = 5.0

type UserId = int
type Pair[T] = tuple[T, T]
type Lookup = dict[str, User]


class Reader(Protocol):
    def read(self) -> bytes: ...


class EchoServer(asyncio.Protocol):
    def data_received(self, data): ...


class ProtocolError(Exception):
    pass


class H2Error(ProtocolError):
    pass


class Color(enum.Enum):
    RED = 1
    green = 2
    X = 3
    _ignore_ = ["tmp"]


class _InternalCache:
    r"""Raw docstring
    over two lines."""

    port: int = 80
    """The port to bind."""

    def withdraw(self, amount):
        self.balance -= amount
        "not a docstring"


class Box[T]:
    def first[S](self, items: list[S], index: dict[str, Order]) -> S:
        return items[0]


class Base:
    def save(self):
        return 1


class Child(Base):
    def save(self):
        return super().save()

    @classmethod
    def build(cls):
        return cls()


def retry(times):
    return lambda fn: fn


@retry(3)
def fetch(repo: "Repo", state: Literal["active", "idle"]) -> "User | None":
    return repo.get(state)


def request(method, url, /, *args, timeout: float = 5.0, **kwargs):
    pass


def kwonly(a, *, strict: bool, b=1):
    pass


def describe(shape):
    match shape:
        case Point(x=0, y=0):
            return "origin"
        case Circle(radius=r) if r > 10:
            return "big"
        case Color.RED:
            return "red"
        case other:
            return str(other)
