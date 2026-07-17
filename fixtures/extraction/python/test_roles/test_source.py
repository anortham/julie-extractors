import unittest


def test_pytest_case():
    assert 1 + 1 == 2


def calculate_total():
    return 2


class ArithmeticTest(unittest.TestCase):
    def setUp(self):
        self.total = calculate_total()

    def tearDown(self):
        self.total = 0

    def test_unittest_case(self):
        self.assertEqual(self.total, 2)

    def production_helper(self):
        return self.total


class OrdinaryHelper:
    def helper(self):
        return 1
