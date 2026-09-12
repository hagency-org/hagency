#!/usr/bin/env python3
"""Privileged qualification only on a disposable GitHub-hosted Linux VM.

Never mounts/delegates/chmods an existing cgroup. Every mutation is relative to
the retained descriptors of one exclusively created random subtree. This helper
is an external test custodian, not the runtime's crash containment implementation.
"""

import argparse
import ctypes
import fcntl
import os
from pathlib import Path
import selectors
import signal
import stat
import subprocess
import sys
import tempfile
import time
import uuid


class Refused(RuntimeError):
    pass


def require(condition, message):
    if not condition:
        raise Refused(message)


def hosted_only(environment, platform, euid):
    require(platform == "linux" and euid == 0, "root on Linux required")
    for key, value in {
        "GITHUB_ACTIONS": "true",
        "RUNNER_ENVIRONMENT": "github-hosted",
        "RUNNER_OS": "Linux",
        "RUNNER_ARCH": "X64",
    }.items():
        require(environment.get(key) == value, f"disposable hosted CI required: {key}")


def filesystem(fd):
    # Linux x86-64 statfs ABI. The only privileged execution target is the X64
    # hosted VM checked above; oversized aligned output storage avoids a guessed
    # Rust/Python struct layout. Linux struct statfs is 120 bytes on this ABI.
    storage = (ctypes.c_long * 64)()
    libc = ctypes.CDLL(None, use_errno=True)
    libc.fstatfs.argtypes = [ctypes.c_int, ctypes.c_void_p]
    libc.fstatfs.restype = ctypes.c_int
    if libc.fstatfs(fd, ctypes.byref(storage)) != 0:
        raise OSError(ctypes.get_errno(), "fstatfs")
    return storage[0]


def initial_namespaces():
    for name, inode, kind in [
        ("user", 0xEFFFFFFD, 0x10000000),
        ("cgroup", 0xEFFFFFFB, 0x02000000),
    ]:
        fd = os.open(f"/proc/thread-self/ns/{name}", os.O_RDONLY | os.O_CLOEXEC)
        try:
            require(filesystem(fd) == 0x6E736673, "namespace is not nsfs")
            require(os.fstat(fd).st_ino == inode, "initial namespace required")
            require(fcntl.ioctl(fd, 0xB703) == kind, "wrong namespace type")
        finally:
            os.close(fd)


def identity(metadata):
    return (metadata.st_dev, metadata.st_ino)


def protected(metadata, directory):
    require(metadata.st_uid == 0 and metadata.st_mode & 0o022 == 0,
            "root-owned nonwritable boundary required")
    require(stat.S_ISDIR(metadata.st_mode) if directory else stat.S_ISREG(metadata.st_mode),
            "unexpected boundary file type")


def populated(data):
    require(len(data) <= 4096, "events observation exceeded bound")
    fields = {}
    for line in data.decode("ascii", "strict").splitlines():
        pair = line.split()
        require(len(pair) == 2 and pair[0] not in fields and pair[1].isdigit(),
                "invalid events observation")
        fields[pair[0]] = pair[1]
    require(fields.get("populated") in ("0", "1"), "missing population observation")
    return fields["populated"] == "1"


def open_control(directory, name, flags):
    fd = os.open(name, flags | os.O_CLOEXEC | os.O_NOFOLLOW, dir_fd=directory)
    try:
        protected(os.fstat(fd), False)
        require(filesystem(fd) == 0x63677270, "control is not cgroup2")
        return fd
    except BaseException:
        os.close(fd)
        raise


def root_mount():
    fd = os.open("/sys/fs/cgroup", os.O_RDONLY | os.O_DIRECTORY | os.O_CLOEXEC | os.O_NOFOLLOW)
    try:
        protected(os.fstat(fd), True)
        require(filesystem(fd) == 0x63677270, "writable cgroup2 delegation unavailable")
        with open(f"/proc/thread-self/fdinfo/{fd}", "rb") as info:
            data = info.read(4097)
        require(len(data) <= 4096, "fdinfo exceeded bound")
        mount_ids = [v.split()[1] for v in data.splitlines() if v.startswith(b"mnt_id:")]
        require(len(mount_ids) == 1, "ambiguous mount identity")
        with open("/proc/thread-self/mountinfo", "rb") as info:
            data = info.read(256 * 1024 + 1)
        require(len(data) <= 256 * 1024, "mountinfo exceeded bound")
        entries = [v.split() for v in data.splitlines() if v.split()[0] == mount_ids[0]]
        require(len(entries) == 1 and entries[0][3] == b"/", "full cgroup mount required")
        split = entries[0].index(b"-")
        require(entries[0][split + 1] == b"cgroup2", "wrong mount filesystem")
        for name in ("cgroup.procs", "cgroup.threads"):
            control = open_control(fd, name, os.O_RDONLY)
            os.close(control)
        return fd
    except BaseException:
        os.close(fd)
        raise


class Group:
    """Only ever constructed for a newly created directory, never an old path."""

    def __init__(self, parent, name, registry):
        require(name.startswith("hagency-ci-") or name in MODES, "unowned cgroup name")
        self.parent, self.name = parent, name
        self.directory = self.events = self.kill = self.procs = None
        self.created_identity = None
        os.mkdir(name, 0o755, dir_fd=parent)  # EEXIST is a refusal, never adoption.
        registry.append(self)  # Cleanup sees partial construction too.
        self.created_identity = identity(os.stat(name, dir_fd=parent, follow_symlinks=False))
        self.directory = os.open(name, os.O_RDONLY | os.O_DIRECTORY | os.O_CLOEXEC | os.O_NOFOLLOW,
                                 dir_fd=parent)
        require(identity(os.fstat(self.directory)) == self.created_identity, "cgroup was replaced")
        require(filesystem(self.directory) == 0x63677270, "created directory is not cgroup2")
        os.fchmod(self.directory, 0o755)
        self.events = open_control(self.directory, "cgroup.events", os.O_RDONLY)
        self.kill = open_control(self.directory, "cgroup.kill", os.O_WRONLY)
        self.procs = open_control(self.directory, "cgroup.procs", os.O_WRONLY)
        threads = open_control(self.directory, "cgroup.threads", os.O_WRONLY)
        try:
            for fd, mode in [(self.kill, 0o200), (self.procs, 0o600), (threads, 0o600)]:
                os.fchmod(fd, mode)
        finally:
            os.close(threads)
        require(not self.is_populated(), "new cgroup was populated externally")

    def is_populated(self):
        require(self.events is not None, "population observation unavailable")
        return populated(os.pread(self.events, 4097, 0))

    def stop(self):
        require(self.kill is not None, "independent kill capability unavailable")
        require(os.write(self.kill, b"1") == 1, "incomplete independent kill write")
        until = time.monotonic() + 5
        while self.is_populated():
            require(time.monotonic() < until, "independent cgroup cleanup timed out")
            time.sleep(0.005)

    def remove(self):
        # Never walk arbitrary descendants, follow symlinks, or remove replacements.
        current = os.stat(self.name, dir_fd=self.parent, follow_symlinks=False)
        require(self.created_identity is not None and stat.S_ISDIR(current.st_mode)
                and identity(current) == self.created_identity,
                "refusing removal of changed cgroup identity")
        if self.events is not None:
            require(not self.is_populated(), "refusing removal of populated cgroup")
        os.rmdir(self.name, dir_fd=self.parent)

    def close(self):
        for fd in (self.procs, self.kill, self.events, self.directory):
            if fd is not None:
                os.close(fd)


MODES = ("guardian-death", "stop", "failed-spawn", "guarantee-refused", "nested-user",
         "nested-cgroup", "custodians-abort")

PROBE_NAMES = ("hagency-cgroup-probe", "hagency-platform-probe")
MAX_PROBE_BYTES = 128 * 1024 * 1024


def copy_stamp(metadata):
    return (identity(metadata), metadata.st_size, metadata.st_mtime_ns, metadata.st_ctime_ns)


def copy_fixed_probe(source, name, directory, created_files):
    require(name in PROBE_NAMES, "fixed staged probe name required")
    source_fd = os.open(source, os.O_RDONLY | os.O_CLOEXEC | os.O_NOFOLLOW | os.O_NONBLOCK)
    destination = None
    try:
        before = os.fstat(source_fd)
        require(stat.S_ISREG(before.st_mode) and before.st_mode & 0o111,
                "fixed probe must be a regular executable")
        require(0 < before.st_size <= MAX_PROBE_BYTES, "probe copy exceeds byte bound")
        destination = os.open(name, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_CLOEXEC | os.O_NOFOLLOW,
                              0o600, dir_fd=directory)
        created_files[name] = identity(os.fstat(destination))
        remaining = before.st_size
        while remaining:
            block = os.read(source_fd, min(64 * 1024, remaining))
            require(block, "probe source shrank during copy")
            remaining -= len(block)
            pending = memoryview(block)
            while pending:
                written = os.write(destination, pending)
                require(written > 0, "incomplete staged probe write")
                pending = pending[written:]
        require(not os.read(source_fd, 1), "probe source grew during copy")
        require(copy_stamp(os.fstat(source_fd)) == copy_stamp(before)
                and copy_stamp(os.stat(source, follow_symlinks=False)) == copy_stamp(before),
                "probe source changed during copy")
        os.fchmod(destination, 0o555)
        after = os.fstat(destination)
        require(stat.S_ISREG(after.st_mode) and after.st_uid == os.geteuid()
                and after.st_nlink == 1 and after.st_size == before.st_size
                and stat.S_IMODE(after.st_mode) == 0o555,
                "staged probe was not exclusively owned and sealed")
    finally:
        if destination is not None:
            os.close(destination)
        os.close(source_fd)


class StagedProbes:
    """Two fixed CI binaries, sealed in an exclusively created boundary.

    Production calls this only after hosted/root admission with fixed /tmp.
    Tests may supply an ordinary owned parent; that is not privileged evidence.
    No source/checkout/ancestor permission is changed. The retained directory
    and exact created files are the only cleanup authority; no recursive walk.
    """

    def __init__(self, probe, parent=Path("/tmp")):
        self.parent = self.directory = None
        self.created_identity = None
        self.files = {}
        self.name = f"hagency-cgroup-binaries-{uuid.uuid4().hex}"
        self.path = Path(parent) / self.name
        require(Path(probe).name == PROBE_NAMES[0], "fixed offline probe name required")
        try:
            self.parent = os.open(parent, os.O_RDONLY | os.O_DIRECTORY | os.O_CLOEXEC | os.O_NOFOLLOW)
            meta = os.fstat(self.parent)
            require(meta.st_uid == os.geteuid() and stat.S_ISDIR(meta.st_mode),
                    "owned staging parent required")
            require(meta.st_mode & 0o022 == 0 or meta.st_mode & stat.S_ISVTX,
                    "writable staging parent requires sticky protection")
            if Path(parent) == Path("/tmp"):
                root_meta = os.stat("/")
                protected(root_meta, True)
                require(meta.st_mode & 0o001 and root_meta.st_mode & 0o001,
                        "fixed staging ancestry must be traversable")
            os.mkdir(self.name, 0o700, dir_fd=self.parent)  # EEXIST never adopts.
            self.created_identity = identity(os.stat(self.name, dir_fd=self.parent, follow_symlinks=False))
            self.directory = os.open(self.name, os.O_RDONLY | os.O_DIRECTORY | os.O_CLOEXEC | os.O_NOFOLLOW,
                                     dir_fd=self.parent)
            self.check_directory()
            for name in PROBE_NAMES:
                copy_fixed_probe(Path(probe).with_name(name), name, self.directory, self.files)
            os.fchmod(self.directory, 0o555)
            require(stat.S_IMODE(os.fstat(self.directory).st_mode) == 0o555,
                    "staged directory was not sealed and traversable")
        except BaseException:
            self.close()
            raise

    def check_directory(self):
        current = os.stat(self.name, dir_fd=self.parent, follow_symlinks=False)
        require(self.created_identity is not None and stat.S_ISDIR(current.st_mode)
                and identity(current) == self.created_identity and current.st_uid == os.geteuid(),
                "refusing changed staging directory")
        if self.directory is not None:
            require(identity(os.fstat(self.directory)) == self.created_identity,
                    "retained staging directory changed")

    def close(self):
        try:
            if self.created_identity is None:
                return
            self.check_directory()
            if self.directory is not None:
                # Ordinary unprivileged tests need write restored for unlink.
                # This is only the exact fresh retained directory, never /tmp,
                # the checkout, a source ancestor or a replacement entry.
                os.fchmod(self.directory, 0o700)
                for name, observed in self.files.items():
                    current = os.stat(name, dir_fd=self.directory, follow_symlinks=False)
                    require(stat.S_ISREG(current.st_mode) and identity(current) == observed,
                            "refusing changed staged probe")
                    os.unlink(name, dir_fd=self.directory)
            os.rmdir(self.name, dir_fd=self.parent)
            self.created_identity = None
        finally:
            for descriptor in (self.directory, self.parent):
                if descriptor is not None:
                    os.close(descriptor)
            self.directory = self.parent = None

    def __enter__(self):
        return self.path / PROBE_NAMES[0]

    def __exit__(self, kind, value, traceback):
        self.close()


def collect(child, until):
    # Drain both bounded streams on this one thread; no reader threads/tasks.
    output = [bytearray(), bytearray()]
    with selectors.DefaultSelector() as selector:
        for index, stream in enumerate((child.stdout, child.stderr)):
            os.set_blocking(stream.fileno(), False)
            selector.register(stream, selectors.EVENT_READ, index)
        while selector.get_map() or child.poll() is None:
            require(time.monotonic() < until, "fixture deadline expired")
            for key, _ in selector.select(min(0.05, max(0, until - time.monotonic()))):
                chunk = os.read(key.fileobj.fileno(), 4096)
                if not chunk:
                    selector.unregister(key.fileobj)
                else:
                    require(len(output[key.data]) + len(chunk) <= 64 * 1024,
                            "fixture output exceeded bound")
                    output[key.data].extend(chunk)
        return child.wait(timeout=0), bytes(output[0]), bytes(output[1])


def owned_fixture(command, work, group, inspect, timeout=20):
    # Private helper, never a service endpoint. The sole production caller below
    # constructs a fixed offline probe command; local tests use owned subprocesses.
    child = subprocess.Popen(command, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
                             stderr=subprocess.PIPE, close_fds=True,
                             pass_fds=(group.directory, group.procs, group.kill),
                             cwd=work, env={"PATH": "/usr/bin:/bin", "LANG": "C"})
    pidfd = None
    try:
        # Retained child has not been reaped, so this initial handle acquisition
        # cannot name a reused PID. Cleanup signals exclusively through pidfd.
        pidfd = os.pidfd_open(child.pid, 0)
        inspect(collect(child, time.monotonic() + timeout))
    finally:
        try:
            group.stop()  # Independent retained root authority, even after host failure.
        finally:
            try:
                if child.poll() is None:
                    require(pidfd is not None, "helper cleanup unknown: pidfd unavailable")
                    try:
                        signal.pidfd_send_signal(pidfd, signal.SIGKILL)
                    except ProcessLookupError:
                        pass  # Observe the retained child's exit; never signal a PID.
                    child.wait(timeout=5)
            finally:
                if pidfd is not None:
                    os.close(pidfd)
                child.stdout.close()
                child.stderr.close()


def reproduce_private_ancestor(probe, work, group, uid, gid):
    """Controlled hosted fault, explicitly NOT a namespace qualification.

    The failed historical log did not capture source-ancestor permissions. This
    new owned0700 ancestor reproduces a consistent mechanism without changing
    any historical checkout/source directory or accepting126 as native refusal.
    """
    parent = os.open(work, os.O_RDONLY | os.O_DIRECTORY | os.O_CLOEXEC | os.O_NOFOLLOW)
    directory = None
    created_identity = None
    copies = {}
    name = "private-probe-source"
    try:
        os.mkdir(name, 0o700, dir_fd=parent)
        created_identity = identity(os.stat(name, dir_fd=parent, follow_symlinks=False))
        directory = os.open(name, os.O_RDONLY | os.O_DIRECTORY | os.O_CLOEXEC | os.O_NOFOLLOW,
                            dir_fd=parent)
        require(identity(os.fstat(directory)) == created_identity, "private fixture directory changed")
        copy_fixed_probe(probe, PROBE_NAMES[0], directory, copies)
        os.fchown(directory, uid, gid)
        os.fchmod(directory, 0o700)
        command = ["/usr/bin/unshare", "--user", "--map-root-user", "--",
                   str(Path(work) / name / PROBE_NAMES[0]), "guardian-death", str(work),
                   str(group.directory), str(group.procs), str(group.kill)]

        def inspect(outcome):
            code, stdout, stderr = outcome
            require(code == 126 and not stdout and b"Permission denied" in stderr,
                    f"controlled private-ancestor permission refusal not observed: exit={code}, {stderr!r}")
        owned_fixture(command, work, group, inspect)
    finally:
        try:
            if created_identity is not None:
                current = os.stat(name, dir_fd=parent, follow_symlinks=False)
                require(stat.S_ISDIR(current.st_mode) and identity(current) == created_identity,
                        "refusing changed private fixture directory")
                for filename, observed in copies.items():
                    current = os.stat(filename, dir_fd=directory, follow_symlinks=False)
                    require(stat.S_ISREG(current.st_mode) and identity(current) == observed,
                            "refusing changed private fixture binary")
                    os.unlink(filename, dir_fd=directory)
                os.rmdir(name, dir_fd=parent)
        finally:
            if directory is not None:
                os.close(directory)
            os.close(parent)
    print("controlled 0700 ancestor exec refusal reproduced; NOT namespace qualification", flush=True)


def run_fixture(probe, mode, work, group, uid, gid):
    base = [str(probe), "guardian-death" if mode.startswith("nested-") else mode,
            str(work), str(group.directory), str(group.procs), str(group.kill)]
    if mode == "nested-user":
        command = ["/usr/bin/unshare", "--user", "--map-root-user", "--", *base]
    elif mode == "nested-cgroup":
        command = ["/usr/bin/unshare", "--cgroup", "--", *base]
    else:
        command = ["/usr/bin/setpriv", "--reuid", str(uid), "--regid", str(gid),
                   "--clear-groups", "--no-new-privs", "--bounding-set=-all",
                   "--inh-caps=-all", "--ambient-caps=-all", "--", *base]

    def inspect(outcome):
        code, stdout, stderr = outcome
        if mode.startswith("nested-"):
            require(code == 78 and not stdout
                    and b"initial Linux user and cgroup namespaces required" in stderr,
                    f"{mode}: actual namespace refusal not observed: exit={code}, {stderr!r}")
        elif mode == "custodians-abort":
            require(code == 87 and not stdout, "abrupt custodian fault not observed")
            require(group.is_populated(), "external cleanup fixture had no surviving execution")
        else:
            require(code == 0 and stdout.startswith(b"qualified:") and not stderr,
                    f"{mode}: qualification failed: exit={code}, {stdout!r}, {stderr!r}")
    owned_fixture(command, work, group, inspect)
    print(f"qualified real Linux fixture: {mode}; independent subtree empty", flush=True)


def run(args):
    hosted_only(os.environ, sys.platform, os.geteuid())
    require(0 < args.uid < 2**32 - 1 and 0 < args.gid < 2**32 - 1, "nonroot fixture identity required")
    initial_namespaces()
    require(hasattr(os, "pidfd_open") and hasattr(signal, "pidfd_send_signal"), "pidfd API required")
    probe = Path(args.probe)
    require(probe.is_absolute() and probe.is_file() and not probe.is_symlink(), "built absolute probe required")
    require(probe.name == "hagency-cgroup-probe", "only the fixed offline probe is supported")
    require(probe.with_name("hagency-platform-probe").is_file(), "offline child fixture missing")
    print(f"cgroup qualification kernel: {os.uname().release}", flush=True)
    root = root_mount()
    registry = []
    errors = []
    try:
        boundary = Group(root, f"hagency-ci-{uuid.uuid4().hex}", registry)
        print(f"created isolated cgroup: {boundary.name}", flush=True)
        # The nested user maps only initial UID0 and cannot traverse private
        # runner-owned build ancestors. Stage fixed bytes under root custody;
        # never make the checkout or its ancestors more accessible.
        with StagedProbes(probe) as staged_probe, tempfile.TemporaryDirectory(prefix="hagency-cgroup-ci-") as scratch:
            os.chown(scratch, args.uid, args.gid)
            for mode in MODES:
                group = Group(boundary.directory, mode, registry)
                work = Path(scratch) / mode
                work.mkdir(mode=0o700)
                os.chown(work, args.uid, args.gid)
                if mode == "nested-user":
                    reproduce_private_ancestor(staged_probe, work, group, args.uid, args.gid)
                run_fixture(staged_probe, mode, work, group, args.uid, args.gid)
    finally:
        # Root capability survives every helper. Kill the one complete subtree
        # first; attempt all owned removals even when an earlier check failed.
        for group in registry:
            if group.kill is not None and group.events is not None:
                try:
                    group.stop()
                except Exception as error:
                    errors.append(str(error))
        for group in reversed(registry):
            try:
                group.remove()
            except Exception as error:
                errors.append(str(error))
            finally:
                group.close()
        os.close(root)
        require(not errors, f"independent root cleanup unresolved: {errors}")
    print("Linux cgroup qualification complete; owned subtree removed", flush=True)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--probe", required=True)
    parser.add_argument("--uid", type=int, required=True)
    parser.add_argument("--gid", type=int, required=True)
    try:
        run(parser.parse_args())
    except Exception as error:
        print(f"cgroup qualification FAILED (never skipped): {error}", file=sys.stderr)
        sys.exit(1)
