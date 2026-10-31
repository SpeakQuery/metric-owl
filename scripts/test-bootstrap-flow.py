#!/usr/bin/env python3
"""Run the actual bootstrap shell flow against disposable release fixtures.

This is orchestration coverage, not an OS, TLS, or application installation test.
On macOS and Linux alike, only /etc/os-release is redirected in a temporary copy
of bootstrap-linux.sh. PATH supplies root/Linux identity, offline GitHub assets,
and fail-closed service/package-management stubs. Real Bash, checksum validation,
release-metadata validation, sequencing, and failure handling still execute.
Downloaded installation/configuration helpers are fixtures; their real behavior
is covered separately by test-https-installer.py and installer acceptance tests.
No SSH, network access, application data, or production services are used.
"""

import hashlib
import json
import os
from pathlib import Path
import re
import shlex
import shutil
import subprocess
import sys
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[1]
BOOTSTRAP = ROOT / "scripts/bootstrap-linux.sh"
SUCCESS = "Installation complete [isolated fixture]"

HELPER = r'''
import json, os, pathlib, sys
state_path = pathlib.Path(os.environ["FLOW_STATE"])
state = json.loads(state_path.read_text())
mode, *args = sys.argv[1:]
def save():
    state_path.write_text(json.dumps(state))
def event(name):
    state["events"].append(name)
    save()
if mode == "install":
    event("install")
    assert os.environ.get("GUARD_INSTALL_QUIET") == "1"
    assert len(args) == 1
    assert (pathlib.Path(args[0]) / "server-guard").read_text() == "fixture binary\n"
    if os.environ.get("FLOW_INSTALL_FAIL") == "1":
        sys.exit("Fixture installation failed")
elif mode == "configure" and args == ["--prepare-admin"]:
    event("admin")
    if state["admin"] is not None:
        print("Existing administrator retained [isolated fixture]")
    elif os.environ.get("FLOW_ADMIN_FAIL") == "1":
        sys.exit("Fixture administrator setup failed")
    else:
        state["admin"] = {"username": "fixture-admin", "mfa_enabled": False}
        save()
elif mode == "configure" and not args:
    event("https")
    if os.environ.get("FLOW_HTTPS_FAIL") == "1":
        sys.exit("Fixture certificate validation failed")
    state["https"] = True
    save()
elif mode == "summary":
    event("summary")
    assert args == ["/var/lib/server-guard", "ssh", "4877", "4242"]
    print("Installation complete [isolated fixture]")
else:
    sys.exit("Unexpected fixture helper invocation")
'''

COMMAND = r'''
import os, pathlib, shutil, sys, tempfile
name = pathlib.Path(sys.argv[0]).name
args = sys.argv[1:]
if name == "id" and args == ["-u"]:
    print("0")
elif name == "uname" and args in (["-s"], ["-m"]):
    print("Linux" if args == ["-s"] else "x86_64")
elif name == "dpkg-query" and args == ["-W", "-f=${Status}", "ca-certificates"]:
    print("install ok installed")
elif name == "systemctl" and args == ["show", "server-guard", "-p", "MainPID", "--value"]:
    print("4242")
elif name == "mktemp" and args == ["-d"]:
    print(tempfile.mkdtemp(prefix="download-", dir=os.environ["TMPDIR"]))
elif name == "curl":
    prefix = os.environ["FLOW_RELEASE_URL"] + "/"
    urls = [arg for arg in args if arg.startswith("https://")]
    assert len(urls) == 1 and urls[0].startswith(prefix), "Non-fixture URL rejected"
    filename = urls[0][len(prefix):]
    assert filename and pathlib.Path(filename).name == filename
    assert args[args.index("--proto") + 1] == "=https"
    assert args[args.index("--proto-redir") + 1] == "=https"
    target = pathlib.Path(args[args.index("--output") + 1]).resolve()
    assert target.is_relative_to(pathlib.Path(os.environ["TMPDIR"]).resolve())
    shutil.copyfile(pathlib.Path(os.environ["FLOW_ASSETS"]) / filename, target)
else:
    sys.exit("Forbidden or unexpected external command: " + name)
'''


class BootstrapFlowTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="metric owl bootstrap flow ")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.bin = self.root / "bin"
        self.assets = self.root / "assets"
        self.stage = self.root / "stage"
        for path in (self.bin, self.assets, self.stage):
            path.mkdir()

        source = BOOTSTRAP.read_text()
        self.assertEqual(source.count("/etc/os-release"), 2,
                         "Review platform fixture substitution after bootstrap changes")
        versions = re.findall(r"https://github\.com/SpeakQuery/metric-owl/releases/download/(v[\w.-]+)", source)
        self.assertEqual(len(versions), 1)
        self.version = versions[0]
        self.release_url = "https://github.com/SpeakQuery/metric-owl/releases/download/" + self.version
        os_release = self.root / "os-release"
        os_release.write_text('ID=ubuntu\nVERSION_ID="24.04"\n')
        self.script = self.root / "bootstrap-linux.sh"
        self.script.write_text(source.replace("/etc/os-release", shlex.quote(str(os_release))))

        # Use a closed PATH: unexpected package/service mutations cannot fall
        # through to host binaries, and curl never accesses the network.
        for name in ("bash", "awk", "grep", "sha256sum", "mkdir", "mv", "rm"):
            actual = shutil.which(name)
            self.assertIsNotNone(actual, f"Test prerequisite is missing: {name}")
            (self.bin / name).symlink_to(actual)
        (self.bin / "python3").symlink_to(sys.executable)
        for name in ("id", "uname", "dpkg-query", "curl", "ss", "runuser", "mktemp",
                     "systemctl", "install", "useradd", "apt-get", "sudo"):
            self.executable(self.bin / name, "#!/usr/bin/env python3\n" + COMMAND)

        self.helper = self.root / "helper.py"
        self.helper.write_text(HELPER)
        for name, mode in (("install-linux.sh", "install"), ("show-setup.sh", "summary")):
            (self.assets / name).write_text(
                '#!/usr/bin/env bash\nset -euo pipefail\n'
                f'exec python3 "$FLOW_HELPER" {mode} "$@"\n')
        (self.assets / "configure-https.py").write_text(
            'import os, runpy, sys\n'
            'sys.argv = [sys.argv[0], "configure", *sys.argv[1:]]\n'
            'runpy.run_path(os.environ["FLOW_HELPER"], run_name="__main__")\n')
        (self.assets / "server-guard-linux-amd64").write_text("fixture binary\n")
        for name in ("uninstall-linux.sh", "connection-hints.sh", "server-guard.service",
                     "LICENSE", "BINARY-LICENSE.txt", "THIRD-PARTY-NOTICES.txt"):
            (self.assets / name).write_text("isolated fixture\n")
        self.metadata = {"version": self.version, "source_commit": "a" * 40,
                         "os": "linux", "architecture": "x86_64", "libc": "musl",
                         "linkage": "static", "platforms": ["ubuntu-24.04"]}
        (self.assets / "release-metadata.json").write_text(json.dumps(self.metadata))
        self.checksums()
        self.state_path = self.root / "state.json"
        self.initial_state = {"events": [], "admin": None, "https": False,
                              "history": [{"time": 123, "cpu": 42}]}
        self.state_path.write_text(json.dumps(self.initial_state))
        self.env = {"PATH": str(self.bin), "HOME": str(self.root), "TMPDIR": str(self.stage),
                    "LC_ALL": "C", "FLOW_HELPER": str(self.helper),
                    "FLOW_STATE": str(self.state_path), "FLOW_ASSETS": str(self.assets),
                    "FLOW_RELEASE_URL": self.release_url}

    @staticmethod
    def executable(path, text):
        path.write_text(text)
        path.chmod(0o700)

    def checksums(self):
        lines = [f"{hashlib.sha256(path.read_bytes()).hexdigest()}  {path.name}\n"
                 for path in sorted(self.assets.iterdir()) if path.name != "SHA256SUMS"]
        (self.assets / "SHA256SUMS").write_text("".join(lines))

    def run_bootstrap(self, **environment):
        result = subprocess.run([str(self.bin / "bash"), str(self.script)],
                                env={**self.env, **environment}, stdin=subprocess.DEVNULL,
                                capture_output=True, text=True, timeout=20)
        self.assertEqual(list(self.stage.iterdir()), [], "Bootstrap stage was not cleaned")
        return result, json.loads(self.state_path.read_text())

    def test_admin_precedes_https_and_summary(self):
        result, state = self.run_bootstrap()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(state["events"], ["install", "admin", "https", "summary"])
        self.assertLess(result.stdout.index("Step 1 of 2"), result.stdout.index("Step 2 of 2"))
        self.assertIn(SUCCESS, result.stdout)
        self.assertEqual(state["history"], self.initial_state["history"])

    def test_admin_failure_stops_before_https_and_success(self):
        result, state = self.run_bootstrap(FLOW_ADMIN_FAIL="1")
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(state["events"], ["install", "admin"])
        self.assertIsNone(state["admin"])
        self.assertIn("SETUP INCOMPLETE: Administrator setup did not finish.", result.stderr)
        self.assertNotIn("Step 2 of 2", result.stdout)
        self.assertNotIn(SUCCESS, result.stdout + result.stderr)

    def test_https_failure_preserves_admin_and_retry_delegates_existing_account(self):
        failed, state = self.run_bootstrap(FLOW_HTTPS_FAIL="1")
        self.assertNotEqual(failed.returncode, 0)
        self.assertEqual(state["events"], ["install", "admin", "https"])
        self.assertIsNotNone(state["admin"])
        self.assertFalse(state["https"])
        self.assertIn("SETUP INCOMPLETE:", failed.stderr)
        self.assertIn("browser address is not ready", failed.stderr)
        self.assertIn("account you created, settings and history are preserved", failed.stderr)
        self.assertNotIn(SUCCESS, failed.stdout + failed.stderr)
        existing_admin = state["admin"].copy()
        retried, after = self.run_bootstrap()
        self.assertEqual(retried.returncode, 0, retried.stderr)
        self.assertEqual(after["events"], ["install", "admin", "https", "install", "admin", "https", "summary"])
        self.assertEqual(after["admin"], existing_admin)
        self.assertEqual(after["history"], self.initial_state["history"])
        self.assertIn("Existing administrator retained", retried.stdout)

    def test_existing_account_and_mfa_are_delegated_to_admin_helper(self):
        state = {**self.initial_state, "admin": {"username": "existing-owner", "mfa_enabled": True}}
        self.state_path.write_text(json.dumps(state))
        result, after = self.run_bootstrap()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(after["events"], ["install", "admin", "https", "summary"])
        self.assertEqual(after["admin"], state["admin"])
        self.assertEqual(after["history"], state["history"])

    def test_install_failure_stops_before_account_and_https(self):
        result, state = self.run_bootstrap(FLOW_INSTALL_FAIL="1")
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(state["events"], ["install"])
        self.assertNotIn(SUCCESS, result.stdout + result.stderr)

    def test_corrupt_download_never_runs_installation(self):
        with (self.assets / "configure-https.py").open("a") as stream:
            stream.write("# corrupted after checksum generation\n")
        result, state = self.run_bootstrap()
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(state["events"], [])
        self.assertIn("FAILED", result.stdout + result.stderr)

    def test_wrong_release_metadata_never_runs_installation(self):
        (self.assets / "release-metadata.json").write_text(json.dumps({**self.metadata, "version": "v0.0.0"}))
        self.checksums()
        result, state = self.run_bootstrap()
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(state["events"], [])
        self.assertIn("Release provenance or platform metadata", result.stderr)

    def test_explicit_local_only_skips_https_but_still_prepares_account(self):
        result, state = self.run_bootstrap(GUARD_CONFIGURE_HTTPS="0")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(state["events"], ["install", "admin", "summary"])
        self.assertFalse(state["https"])


if __name__ == "__main__":
    unittest.main(verbosity=2)
