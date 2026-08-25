import contextlib
import functools
import unittest

import pytest


@pytest.fixture
def arithmetic_operands():
    return (2, 3)


@contextlib.contextmanager
def temporary_operands():
    yield (2, 3)


@pytest.mark.parametrize("left,right", [(1, 1), (2, 3)])
def test_addition_is_commutative(left, right):
    assert left + right == right + left


@functools.lru_cache(maxsize=None)
def cached_total():
    return 2


@pytest.mark.asyncio
async def test_async_total():
    assert await load_total() == 2


async def build_payload():
    return {"total": 2}


async def load_total():
    return 2


def calculate_total():
    return 2


class TestArithmetic:
    def setup_method(self, method):
        self.total = calculate_total()

    def teardown_method(self, method):
        self.total = 0

    def setup_client(self):
        return object()

    def test_total_is_two(self):
        assert self.total == 2


class ArithmeticTest(unittest.TestCase):
    def setUp(self):
        self.total = calculate_total()

    def tearDown(self):
        self.total = 0

    def testAdditionInPlace(self):
        self.assertEqual(self.total + 1, 3)

    @unittest.skipIf(True, "environment guard")
    def test_guarded_case(self):
        self.assertEqual(self.total, 2)

    def production_helper(self):
        return self.total


class OrdinaryHelper:
    def helper(self):
        return 1
