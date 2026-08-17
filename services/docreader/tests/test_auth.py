import unittest

from docreader.auth import _authorization_token


class AuthorizationTokenTest(unittest.TestCase):
    def test_strips_bearer_from_str(self):
        self.assertEqual(_authorization_token("Bearer secret"), b"secret")

    def test_strips_bearer_from_bytes(self):
        self.assertEqual(_authorization_token(b"Bearer secret"), b"secret")

    def test_case_insensitive_prefix(self):
        self.assertEqual(_authorization_token(b"bearer secret"), b"secret")
        self.assertEqual(_authorization_token("BEARER secret"), b"secret")

    def test_raw_token_without_prefix(self):
        self.assertEqual(_authorization_token("secret"), b"secret")
        self.assertEqual(_authorization_token(b"secret"), b"secret")

    def test_empty_and_unknown(self):
        self.assertEqual(_authorization_token(None), b"")
        self.assertEqual(_authorization_token(""), b"")
        self.assertEqual(_authorization_token(b""), b"")
