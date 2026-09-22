import unittest

import pytest_asyncio
from django.test import TestCase
from rest_framework.test import APITestCase


class UserApiTests(APITestCase):
    def test_list_users(self):
        self.assertTrue(True)


class AsyncTests(unittest.IsolatedAsyncioTestCase):
    async def asyncSetUp(self):
        self.value = 1

    async def test_value(self):
        self.assertEqual(self.value, 1)


class QuestionModelTests(TestCase):
    @classmethod
    def setUpTestData(cls):
        cls.question = None

    def test_was_published_recently(self):
        def test_helper():
            return None

        self.assertIsNone(test_helper())


@pytest_asyncio.fixture
async def client():
    yield None
