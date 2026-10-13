"""Container and HTTP helpers for installed-package tests."""

import os
import subprocess
import time
import uuid
from dataclasses import dataclass
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
HERE = Path(__file__).resolve().parent


def command(*args, input=None, check=True, env=None):
    result = subprocess.run(
        list(map(str, args)),
        input=input,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        env=env,
    )
    if check and result.returncode:
        raise AssertionError(
            f"Command failed ({result.returncode}): {' '.join(map(str, args))}\n"
            f"stdout:\n{result.stdout[-4000:]}\nstderr:\n{result.stderr[-4000:]}"
        )
    return result


def eventually(predicate, description, timeout=60, interval=2):
    deadline = time.monotonic() + timeout
    last_error = None
    while time.monotonic() < deadline:
        try:
            value = predicate()
            if value:
                return value
        except (AssertionError, ValueError) as error:
            last_error = error
        time.sleep(interval)
    raise AssertionError(f"Timed out waiting for {description}: {last_error}")


@dataclass(frozen=True)
class Package:
    path: Path
    kind: str
    arch: str
    version: str

    @property
    def app_version(self):
        return self.version.replace("~", "-", 1)


class Container:
    def __init__(self, engine, image, arch, *, browser=False, replayos=False):
        self.engine = engine
        self.name = f"replay-control-package-{uuid.uuid4().hex[:12]}"
        args = [engine, "run", "-d", "--name", self.name, "--platform", f"linux/{arch}"]
        if replayos:
            args += ["--tmpfs", "/media/sd"]
        if browser:
            if engine == "podman":
                args += ["--network", "podman"]
            else:
                args += ["-p", "127.0.0.1::8080", "-p", "127.0.0.1::8443"]
        command(*args, image, "sleep", "infinity")

    def close(self):
        command(self.engine, "rm", "-f", self.name, check=False)

    def copy(self, source, destination):
        command(self.engine, "cp", source, f"{self.name}:{destination}")

    def exec(self, *args, check=True, input=None, env=None, detached=False):
        prefix = [self.engine, "exec"]
        if input is not None:
            prefix.append("-i")
        if detached:
            prefix.append("-d")
        for key, value in (env or {}).items():
            prefix += ["-e", f"{key}={value}"]
        return command(*prefix, self.name, *args, input=input, check=check)

    def output(self, *args):
        return self.exec(*args).stdout.strip()

    def write(self, path, content):
        self.exec("tee", path, input=content)

    def read(self, path):
        return self.output("cat", path)

    def exists(self, path):
        return self.exec("test", "-e", path, check=False).returncode == 0

    def file_has(self, path, text):
        return text in self.read(path)

    def http(self, path, *, port=8443, method="GET", data=None, headers=(), secure=True, timeout=10):
        scheme = "https" if secure else "http"
        args = ["curl", "-k", "-sS", "--max-time", str(timeout), "-w", "\n%{http_code}", "-X", method]
        for header in headers:
            args += ["-H", header]
        if data is not None:
            args += ["--data", data]
        result = self.exec(*args, f"{scheme}://127.0.0.1:{port}{path}", check=False)
        if result.returncode:
            return 0, result.stderr
        body, status = result.stdout.rsplit("\n", 1)
        return int(status), body

    def wait_http(self, path="/api/version", *, port=8443, expected=200, timeout=60):
        return eventually(
            lambda: (response if (response := self.http(path, port=port))[0] == expected else None),
            f"HTTP {expected} at {path} on port {port}",
            timeout=timeout,
        )[1]

    def browser_url(self):
        if self.engine == "podman":
            address = command(
                self.engine,
                "inspect",
                self.name,
                "--format",
                "{{range .NetworkSettings.Networks}}{{.IPAddress}}{{end}}",
            ).stdout.strip()
            assert address, "container has no browser-accessible IP"
            return f"https://{address}:8443"
        port = command(self.engine, "port", self.name, "8443/tcp").stdout.strip().splitlines()[-1]
        return f"https://127.0.0.1:{port.rsplit(':', 1)[-1]}"

    def browser_test(self, test_file, **variables):
        env = os.environ.copy()
        env.update({"APP_URL": self.browser_url(), **variables})
        command("python3", "-m", "pytest", f"tests/e2e/{test_file}", "-q", env=env)

    def install_shims(self, *, arm64=False):
        targets = {
            "systemctl-shim": ["/usr/bin/systemctl"],
            "systemd-run-shim": ["/usr/local/bin/systemd-run", "/usr/bin/systemd-run"],
        }
        if arm64:
            targets["systemctl-shim"].append("/usr/local/bin/systemctl")
            targets["readlink-shim"] = ["/usr/local/bin/readlink"]
        for source, destinations in targets.items():
            for destination in destinations:
                self.copy(HERE / source, destination)
                self.exec("chmod", "+x", destination)


def build_image(engine, image, dockerfile, arch, *, base=None):
    args = [engine, "build", "--platform", f"linux/{arch}", "-t", image]
    if base:
        args += ["--build-arg", f"BASE={base}"]
    command(*args, "-f", HERE / dockerfile, HERE)


def package_version(container, kind):
    if kind == "rpm":
        return container.output("rpm", "-q", "--qf", "%{VERSION}", "replay-control")
    return container.output("dpkg-query", "-W", "-f=${Version}", "replay-control")


def assert_owned(container, kind, *paths):
    for path in paths:
        if kind == "rpm":
            owner = container.output("rpm", "-qf", "--qf", "%{NAME}", path)
        else:
            owner = container.output("dpkg-query", "-S", path).split(": ", 1)[0].split(":", 1)[0]
        assert owner == "replay-control", (path, owner)
