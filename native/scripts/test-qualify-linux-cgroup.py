#!/usr/bin/env python3
"""Pure/ordinary-file refusal checks; never Linux containment evidence."""
import importlib.util
import contextlib
import io
import os
from pathlib import Path
import signal
import stat
import subprocess
import sys
import tempfile
import time
from types import SimpleNamespace
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location(
    "qualification", Path(__file__).with_name("qualify-linux-cgroup.py"))
qualification = importlib.util.module_from_spec(spec)
spec.loader.exec_module(qualification)


class ProvisionerRefusal(unittest.TestCase):
    def test_hosted_gate(self):
        env = {"GITHUB_ACTIONS": "true", "RUNNER_ENVIRONMENT": "github-hosted",
               "RUNNER_OS": "Linux", "RUNNER_ARCH": "X64"}
        qualification.hosted_only(env, "linux", 0)
        for key in env:
            with self.assertRaises(qualification.Refused):
                qualification.hosted_only({**env, key: "wrong"}, "linux", 0)
        for platform, uid in [("darwin", 0), ("linux", 1000)]:
            with self.assertRaises(qualification.Refused):
                qualification.hosted_only(env, platform, uid)

    def test_population_requires_exact_observation(self):
        self.assertFalse(qualification.populated(b"populated 0\nfrozen 0\n"))
        self.assertTrue(qualification.populated(b"populated 1\n"))
        for data in [b"", b"frozen 0", b"populated 2", b"populated 00",
                     b"populated 0\npopulated 1", b"populated 0 extra", b"x" * 4097]:
            with self.assertRaises(qualification.Refused):
                qualification.populated(data)

    def test_removal_refuses_replaced_directory(self):
        with tempfile.TemporaryDirectory() as root:
            parent = os.open(root, os.O_RDONLY)
            try:
                path = Path(root) / "owned"
                path.mkdir()
                group = qualification.Group.__new__(qualification.Group)
                group.parent, group.name, group.events = parent, "owned", None
                group.created_identity = qualification.identity(path.stat())
                # Keep the original inode allocated so replacement cannot reuse it.
                path.rename(Path(root) / "original")
                path.mkdir()
                with self.assertRaises(qualification.Refused):
                    group.remove()
                self.assertTrue(path.is_dir())
                path.rmdir()
                path.symlink_to(Path(root) / "original", target_is_directory=True)
                with self.assertRaises(qualification.Refused):
                    group.remove()
                self.assertTrue(path.is_symlink())
            finally:
                os.close(parent)

    def test_bounded_collect_failure_with_owned_subprocess(self):
        for code, timeout, expected in [
            ("import time; time.sleep(10)", 0.05, "deadline"),
            ("import os, time; os.write(1, b'x' * (128 * 1024)); time.sleep(10)", 2, "output"),
        ]:
            child = subprocess.Popen([sys.executable, "-c", code], stdout=subprocess.PIPE,
                                     stderr=subprocess.PIPE, stdin=subprocess.DEVNULL)
            try:
                with self.assertRaisesRegex(qualification.Refused, expected):
                    qualification.collect(child, time.monotonic() + timeout)
            finally:
                child.kill()
                child.wait(timeout=5)
                child.stdout.close()
                child.stderr.close()
            self.assertIsNotNone(child.returncode)

    def probe_sources(self, root, data=b"offline native fixture"):
        source = Path(root) / "private-build"
        source.mkdir(mode=0o700)
        for name in qualification.PROBE_NAMES:
            path = source / name
            path.write_bytes(data)
            path.chmod(0o700)
        return source / qualification.PROBE_NAMES[0]

    def test_staging_fixed_bytes_modes_and_native_execution(self):
        with tempfile.TemporaryDirectory() as root:
            # Build a tiny native fixture: copying an Apple platform-signed
            # /bin/echo out of its protected location is killed by macOS AMFI.
            # This fixed ordinary program has no namespace/cgroup operation.
            executable = Path(root) / "native-fixture"
            subprocess.run(["/usr/bin/cc", "-x", "c", "-o", str(executable), "-"],
                           input=b'#include <stdio.h>\nint main(void) { puts("staged-native-fixture"); return 0; }\n',
                           capture_output=True, env={"PATH": "/usr/bin:/bin"},
                           timeout=30, check=True)
            probe = self.probe_sources(root, executable.read_bytes())
            before = {path: (qualification.copy_stamp(path.stat()), stat.S_IMODE(path.stat().st_mode))
                      for path in [probe.parent, probe, probe.with_name(qualification.PROBE_NAMES[1])]}
            # Actual compiled native bytes in the two fixed fixture slots. This
            # is execution/copy evidence, never cgroup or namespace qualification.
            with qualification.StagedProbes(probe, Path(root)) as staged:
                self.assertNotEqual(staged.parent, probe.parent)
                self.assertEqual(stat.S_IMODE(staged.parent.stat().st_mode), 0o555)
                for name in qualification.PROBE_NAMES:
                    target = staged.with_name(name)
                    self.assertEqual(target.read_bytes(), probe.with_name(name).read_bytes())
                    self.assertEqual(stat.S_IMODE(target.stat().st_mode), 0o555)
                    self.assertEqual(target.stat().st_uid, os.geteuid())
                    self.assertEqual(target.stat().st_nlink, 1)
                    result = subprocess.run([str(target), "staged-native-fixture"],
                                            stdin=subprocess.DEVNULL, capture_output=True,
                                            env={}, timeout=5, check=True)
                    self.assertEqual(result.stdout, b"staged-native-fixture\n")
                    self.assertEqual(result.stderr, b"")
            self.assertFalse(staged.parent.exists())
            executable.unlink()
            for path, observed in before.items():
                self.assertEqual((qualification.copy_stamp(path.stat()), stat.S_IMODE(path.stat().st_mode)), observed)

    def test_staging_refuses_symlink_directory_empty_oversized_and_nonexecutables(self):
        for fault in ("symlink", "directory", "empty", "oversized", "mode"):
            with self.subTest(fault=fault), tempfile.TemporaryDirectory() as root:
                probe = self.probe_sources(root)
                bad = probe.with_name(qualification.PROBE_NAMES[1])
                if fault == "symlink":
                    bad.unlink()
                    bad.symlink_to(probe)
                elif fault == "directory":
                    bad.unlink()
                    bad.mkdir()
                elif fault == "empty":
                    bad.write_bytes(b"")
                elif fault == "oversized":
                    with bad.open("wb") as out:
                        out.truncate(qualification.MAX_PROBE_BYTES + 1)
                else:
                    bad.chmod(0o600)
                with self.assertRaises((OSError, qualification.Refused)):
                    qualification.StagedProbes(probe, Path(root))
                self.assertEqual(list(Path(root).iterdir()), [probe.parent])
                self.assertTrue(probe.is_file())

    def test_staging_partial_write_cleanup_and_bound(self):
        with tempfile.TemporaryDirectory() as root:
            probe = self.probe_sources(root)
            with patch.object(qualification.os, "write", return_value=0):
                with self.assertRaisesRegex(qualification.Refused, "incomplete"):
                    qualification.StagedProbes(probe, Path(root))
            self.assertEqual(list(Path(root).iterdir()), [probe.parent])
            with patch.object(qualification, "MAX_PROBE_BYTES", 5):
                with self.assertRaisesRegex(qualification.Refused, "bound"):
                    qualification.StagedProbes(probe, Path(root))
            self.assertEqual(list(Path(root).iterdir()), [probe.parent])

    def test_staging_cleanup_refuses_file_and_directory_replacements(self):
        for fault in ("file", "directory", "symlink"):
            with self.subTest(fault=fault), tempfile.TemporaryDirectory() as root:
                probe = self.probe_sources(root)
                stage = qualification.StagedProbes(probe, Path(root))
                original = Path(root) / "retained-original"
                if fault == "file":
                    os.fchmod(stage.directory, 0o700)
                    target = stage.path / qualification.PROBE_NAMES[0]
                    target.rename(original)
                    target.write_bytes(b"replacement must survive")
                else:
                    stage.path.rename(original)
                    if fault == "directory":
                        stage.path.mkdir()
                    else:
                        stage.path.symlink_to(original, target_is_directory=True)
                    target = stage.path
                with self.assertRaisesRegex(qualification.Refused, "changed stag"):
                    stage.close()
                self.assertTrue(target.exists())
                if fault == "file":
                    self.assertEqual(target.read_bytes(), b"replacement must survive")
                elif fault == "directory":
                    self.assertEqual(list(target.iterdir()), [])
                else:
                    self.assertTrue(target.is_symlink())
                self.assertTrue(original.exists())
                # The fixture owns these temporary test artifacts. Production
                # cleanup above deliberately refused to remove the replacement.
                if original.is_dir():
                    original.chmod(0o700)

    def test_nested_namespace_requires_native_refusal_not_exec_failure(self):
        for mode in ("nested-user", "nested-cgroup"):
            group = SimpleNamespace(directory=3, procs=4, kill=5)
            def inspect_failure(command, work, group, inspect):
                inspect((126, b"", b"unshare: Permission denied"))
            with patch.object(qualification, "owned_fixture", side_effect=inspect_failure):
                with self.assertRaisesRegex(qualification.Refused, "actual namespace refusal not observed"):
                    qualification.run_fixture(Path("/fixed/hagency-cgroup-probe"), mode,
                                              Path("/fixed/work"), group, 1000, 1000)

    def test_controlled_private_ancestor_is_separate_and_cleans_exact_copy(self):
        for code in (126, 78):
            with self.subTest(code=code), tempfile.TemporaryDirectory() as root:
                probe = self.probe_sources(root)
                work = Path(root) / "work"
                work.mkdir(mode=0o700)
                group = SimpleNamespace(directory=3, procs=4, kill=5)
                def observed(command, directory, group, inspect):
                    self.assertEqual(command[:5], ["/usr/bin/unshare", "--user", "--map-root-user", "--",
                                                   str(work / "private-probe-source" / qualification.PROBE_NAMES[0])])
                    private = work / "private-probe-source"
                    self.assertEqual(stat.S_IMODE(private.stat().st_mode), 0o700)
                    self.assertEqual((private / qualification.PROBE_NAMES[0]).read_bytes(), probe.read_bytes())
                    inspect((code, b"", b"unshare: Permission denied"))
                # This verifies construction/custody and result classification
                # only. Actual mapping/exec refusal is a hosted privileged gate.
                output = io.StringIO()
                with patch.object(qualification, "owned_fixture", side_effect=observed), contextlib.redirect_stdout(output):
                    if code == 126:
                        qualification.reproduce_private_ancestor(probe, work, group, os.geteuid(), os.getegid())
                    else:
                        with self.assertRaisesRegex(qualification.Refused, "permission refusal not observed"):
                            qualification.reproduce_private_ancestor(probe, work, group, os.geteuid(), os.getegid())
                self.assertEqual(list(work.iterdir()), [])
                self.assertEqual("NOT namespace qualification" in output.getvalue(), code == 126)

    def test_staging_refuses_source_growth_without_leaving_stage(self):
        with tempfile.TemporaryDirectory() as root:
            probe = self.probe_sources(root)
            real_read = os.read
            changed = False
            def changing(fd, size):
                nonlocal changed
                value = real_read(fd, size)
                if not changed:
                    changed = True
                    with probe.open("ab") as output:
                        output.write(b"source mutation")
                return value
            with patch.object(qualification.os, "read", side_effect=changing):
                with self.assertRaisesRegex(qualification.Refused, "grew"):
                    qualification.StagedProbes(probe, Path(root))
            self.assertEqual(list(Path(root).iterdir()), [probe.parent])

    if sys.platform == "linux":
        # Actual pidfd cleanup only runs on Linux; macOS collector checks above
        # must not be counted as this platform-specific execution evidence.
        def test_linux_finally_cleans_after_timeout_and_output_cap(self):
            for code, timeout, expected in [
                ("import time; time.sleep(10)", 0.05, "deadline"),
                ("import os, time; os.write(2, b'x' * (128 * 1024)); time.sleep(10)", 2, "output"),
            ]:
                with tempfile.TemporaryDirectory() as root:
                    # Ordinary files are a test double for stop invocation only.
                    # No cgroup is read, created or changed by this test.
                    with tempfile.TemporaryFile() as first, tempfile.TemporaryFile() as second, tempfile.TemporaryFile() as third:
                        observations = []
                        group = SimpleNamespace(directory=first.fileno(), procs=second.fileno(),
                                                kill=third.fileno(), stop=lambda: observations.append("stop"))
                        real_popen = subprocess.Popen
                        children = []

                        def spawn(*args, **kwargs):
                            child = real_popen(*args, **kwargs)
                            children.append(child)
                            return child

                        with patch.object(qualification.subprocess, "Popen", side_effect=spawn):
                            with self.assertRaisesRegex(qualification.Refused, expected):
                                qualification.owned_fixture([sys.executable, "-c", code], root, group,
                                                            lambda _: self.fail("unexpected completion"), timeout)
                        self.assertEqual(observations, ["stop"])
                        self.assertEqual(children[0].returncode, -signal.SIGKILL)
                        self.assertTrue(children[0].stdout.closed and children[0].stderr.closed)


if __name__ == "__main__":
    unittest.main()
