"""Tests for the Isolate guest SDK for Python."""

import io
import json
import os
import sys
import unittest
from unittest.mock import patch

# Import the module under test
import isolate_guest


class TestGuestError(unittest.TestCase):
    def test_error_message(self):
        err = isolate_guest.GuestError("something failed")
        self.assertEqual(str(err), "guest error: something failed")
        self.assertEqual(err.message, "something failed")

    def test_error_is_exception(self):
        with self.assertRaises(isolate_guest.GuestError):
            raise isolate_guest.GuestError("test")


class TestReadInput(unittest.TestCase):
    def test_read_valid_json(self):
        data = json.dumps({"name": "test", "value": 42})
        with patch("sys.stdin", io.TextIOWrapper(io.BytesIO(data.encode()))):
            result = isolate_guest.read_input()
        self.assertEqual(result, {"name": "test", "value": 42})

    def test_read_empty_returns_empty_dict(self):
        with patch("sys.stdin", io.TextIOWrapper(io.BytesIO(b""))):
            result = isolate_guest.read_input()
        self.assertEqual(result, {})

    def test_read_invalid_json_raises(self):
        with patch("sys.stdin", io.TextIOWrapper(io.BytesIO(b"not json"))):
            with self.assertRaises(isolate_guest.GuestError):
                isolate_guest.read_input()


class TestReadRaw(unittest.TestCase):
    def test_read_raw_bytes(self):
        with patch("sys.stdin", io.TextIOWrapper(io.BytesIO(b"\x00\x01\x02"))):
            result = isolate_guest.read_raw()
        self.assertEqual(result, b"\x00\x01\x02")


class TestWriteOutput(unittest.TestCase):
    def test_write_dict(self):
        buf = io.StringIO()
        with patch("sys.stdout", buf):
            isolate_guest.write_output({"result": "ok"})
        self.assertEqual(json.loads(buf.getvalue().strip()), {"result": "ok"})

    def test_write_list(self):
        buf = io.StringIO()
        with patch("sys.stdout", buf):
            isolate_guest.write_output([1, 2, 3])
        self.assertEqual(json.loads(buf.getvalue().strip()), [1, 2, 3])

    def test_write_unserializable_raises(self):
        with self.assertRaises(isolate_guest.GuestError):
            isolate_guest.write_output(object())


class TestWriteRaw(unittest.TestCase):
    def test_write_raw_bytes(self):
        buf = io.BytesIO()
        mock_stdout = io.TextIOWrapper(buf)
        with patch("sys.stdout", mock_stdout):
            isolate_guest.write_raw(b"hello raw")
        buf.seek(0)
        self.assertEqual(buf.read(), b"hello raw")


class TestEnvironment(unittest.TestCase):
    def test_get_env_exists(self):
        with patch.dict(os.environ, {"TEST_VAR": "hello"}):
            self.assertEqual(isolate_guest.get_env("TEST_VAR"), "hello")

    def test_get_env_missing(self):
        result = isolate_guest.get_env("DEFINITELY_NOT_SET_12345")
        self.assertIsNone(result)

    def test_get_all_env(self):
        with patch.dict(os.environ, {"A": "1", "B": "2"}, clear=True):
            result = isolate_guest.get_all_env()
        self.assertEqual(result["A"], "1")
        self.assertEqual(result["B"], "2")

    def test_get_args(self):
        with patch("sys.argv", ["prog", "--flag", "value"]):
            result = isolate_guest.get_args()
        self.assertEqual(result, ["prog", "--flag", "value"])


class TestLogging(unittest.TestCase):
    def _capture_stderr(self, func, msg):
        buf = io.StringIO()
        with patch("sys.stderr", buf):
            func(msg)
        return buf.getvalue()

    def test_log_debug(self):
        output = self._capture_stderr(isolate_guest.log_debug, "debug msg")
        self.assertIn("[DEBUG]", output)
        self.assertIn("debug msg", output)

    def test_log_info(self):
        output = self._capture_stderr(isolate_guest.log_info, "info msg")
        self.assertIn("[INFO]", output)

    def test_log_warn(self):
        output = self._capture_stderr(isolate_guest.log_warn, "warn msg")
        self.assertIn("[WARN]", output)

    def test_log_error(self):
        output = self._capture_stderr(isolate_guest.log_error, "error msg")
        self.assertIn("[ERROR]", output)


if __name__ == "__main__":
    unittest.main()
