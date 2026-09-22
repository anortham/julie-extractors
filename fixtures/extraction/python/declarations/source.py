import functools
import os.path
from typing import Annotated, ClassVar, Optional
from sqlalchemy.orm import Mapped
from .base import BaseService, Mixin as AuditMixin
from ..core import *


class Order:
    total: int = 0
    tags: ClassVar[list[str]] = []
    customer: Mapped["Customer"] = None

    def __init__(self, repo: Optional[Repository] = None):
        self.total = 0
        self.lines, self.discount = [], 0
        self._cache: dict[str, int] = {}

    def reset(self) -> "Order":
        self.total = 0
        self._cache = {}
        return self

    @property
    def repository(self) -> Repository | None:
        return self.repo


class OrderService(AuditMixin, BaseService, Order):
    def ranked(self, orders: list[Order], limit: Annotated[int, "max"]) -> list[Order]:
        def weight(order):
            return order.total

        return sorted(orders, key=lambda order: weight(order))[:limit]


def retry(times):
    def decorate(fn):
        @functools.wraps(fn)
        def wrapper(*args, **kwargs):
            return fn(*args, **kwargs)

        return wrapper

    class Attempt:
        pass

    return decorate


def lookup(path: str):
    return os.path.join(path, "orders")
