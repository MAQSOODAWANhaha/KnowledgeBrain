import os
import socket
import unittest
from unittest.mock import patch

from docreader.utils.ssrf import is_ssrf_safe_url, reset_ssrf_whitelist_cache_for_test


class TestSSRFValidation(unittest.TestCase):
    def setUp(self) -> None:
        self._env_patch = patch.dict(
            os.environ,
            {"SSRF_WHITELIST": "", "SSRF_WHITELIST_EXTRA": ""},
            clear=False,
        )
        self._env_patch.start()
        reset_ssrf_whitelist_cache_for_test()

    def tearDown(self) -> None:
        self._env_patch.stop()
        reset_ssrf_whitelist_cache_for_test()

    def test_blocks_loopback_ip(self):
        safe, reason = is_ssrf_safe_url("http://127.0.0.1:8080/page")
        self.assertFalse(safe)
        self.assertTrue(reason)

    def test_blocks_restricted_hostname(self):
        safe, reason = is_ssrf_safe_url("http://host.docker.internal/secret")
        self.assertFalse(safe)
        self.assertIn("restricted", reason)

    def test_blocks_metadata_host(self):
        safe, reason = is_ssrf_safe_url(
            "http://169.254.169.254/latest/meta-data/iam/security-credentials/"
        )
        self.assertFalse(safe)
        self.assertTrue(reason)

    @patch("docreader.utils.ssrf.socket.getaddrinfo")
    def test_allows_public_https(self, resolve):
        resolve.return_value = [
            (socket.AF_INET, socket.SOCK_STREAM, socket.IPPROTO_TCP, "", ("93.184.216.34", 0))
        ]
        safe, reason = is_ssrf_safe_url("https://example.com/article")
        self.assertTrue(safe, reason)
        self.assertEqual(reason, "")
        resolve.assert_called_once_with("example.com", None, type=socket.SOCK_STREAM)

    @patch("docreader.utils.ssrf.socket.getaddrinfo")
    def test_rejects_dns_failure(self, resolve):
        resolve.side_effect = socket.gaierror("test resolver unavailable")
        safe, reason = is_ssrf_safe_url("https://example.com/article")
        self.assertFalse(safe)
        self.assertIn("DNS resolution failed", reason)

    @patch("docreader.utils.ssrf.socket.getaddrinfo")
    def test_rejects_public_hostname_with_any_private_answer(self, resolve):
        resolve.return_value = [
            (socket.AF_INET, socket.SOCK_STREAM, socket.IPPROTO_TCP, "", (address, 0))
            for address in ["93.184.216.34", "127.0.0.1"]
        ]
        safe, reason = is_ssrf_safe_url("https://example.com/article")
        self.assertFalse(safe)
        self.assertTrue(reason)


if __name__ == "__main__":
    unittest.main()
