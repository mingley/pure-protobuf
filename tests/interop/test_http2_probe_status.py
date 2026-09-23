"""Server probes must verify the actual HTTP/2 status, not just HEADERS."""

import unittest

from scripts.http2_probe_status import decode_first_status


def headers(payload: bytes, flags: int = 0x04) -> tuple[int, int, int, bytes]:
    return (1, flags, 1, payload)


class Http2ProbeStatusTest(unittest.TestCase):
    def test_pinned_peer_huffman_statuses_and_plain_fallback(self):
        self.assertEqual(decode_first_status(headers(bytes.fromhex("4883680dff5684d7ab76ff"))), 405)
        self.assertEqual(decode_first_status(headers(bytes.fromhex("4883682dff"))), 415)
        self.assertEqual(decode_first_status(headers(b"\x48\x03" + b"405")), 405)

    def test_wrong_status_and_incomplete_headers_are_not_success(self):
        self.assertEqual(decode_first_status(headers(b"\x88")), 200)
        self.assertNotEqual(decode_first_status(headers(b"\x88")), 405)
        for payload, flags in [
            (b"", 0x04),
            (b"\x48\x83\x68\x0d", 0x04),
            (b"\x48\x83\x68\x0d\xff", 0x00),
            (b"\x48\x83\xff\xff\xff\xff\xff", 0x04),
            (b"\x48\x83\x00\x00\x00", 0x04),
        ]:
            with self.subTest(payload=payload, flags=flags):
                with self.assertRaises(ValueError):
                    decode_first_status(headers(payload, flags))


if __name__ == "__main__":
    unittest.main()
