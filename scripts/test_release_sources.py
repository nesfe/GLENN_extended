"""A timed-out source download must resume and never publish partial bytes."""
import hashlib
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

from release_sources import download


class DownloadTests(unittest.TestCase):
    def test_resumes_after_timeout_then_verifies_before_rename(self):
        with tempfile.TemporaryDirectory() as directory:
            target = Path(directory) / 'source.tgz'
            partial = target.with_name(target.name + '.part')
            calls = []

            def curl(args):
                self.assertFalse(target.exists())
                self.assertEqual(args[args.index('--continue-at') + 1], '-')
                self.assertEqual(args[-1], str(partial))
                calls.append(args)
                if len(calls) == 1:
                    partial.write_bytes(b'first')
                    return subprocess.CompletedProcess(args, 28)
                self.assertEqual(partial.read_bytes(), b'first')
                partial.write_bytes(b'firstsecond')
                return subprocess.CompletedProcess(args, 0)

            digest = hashlib.sha256(b'firstsecond').hexdigest()
            with patch('release_sources.subprocess.run', side_effect=curl):
                download(target, 'https://example.invalid/source.tgz', digest)
                download(target, 'https://example.invalid/source.tgz', digest)
            self.assertEqual(len(calls), 2)
            self.assertEqual(target.read_bytes(), b'firstsecond')
            self.assertFalse(partial.exists())

    def test_wrong_checksum_never_becomes_a_release_asset(self):
        with tempfile.TemporaryDirectory() as directory:
            target = Path(directory) / 'source.tgz'
            partial = target.with_name(target.name + '.part')
            partial.write_bytes(b'corrupt')
            with patch('release_sources.subprocess.run',
                       return_value=subprocess.CompletedProcess([], 0)):
                with self.assertRaisesRegex(RuntimeError, 'checksum mismatch'):
                    download(target, 'https://example.invalid/source.tgz', 'wrong')
            self.assertFalse(target.exists())
            self.assertFalse(partial.exists())


if __name__ == '__main__':
    unittest.main()
