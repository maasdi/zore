import argparse
import http.client
import json
from pathlib import Path
import socket
import subprocess
import tempfile
import time
import uuid


HOMEPAGE = b'<!doctype html><html><link rel="stylesheet" href="/site.css"><body>Zore release smoke</body></html>\n'
STYLESHEET = b"body { color: #123456; }\n"
BINARY = bytes(range(256)) + b"\x00\xff\r\n"
SOURCE = '''package main

import "std/errors"
import "std/net/http"
import "std/os"
import "std/strconv"
import "std/time"

func asset(w mut http.ResponseWriter, path string, contentType string) error {
    let data = os.ReadFile(path)?
    w.SetHeader("Content-Type", contentType)?
    w.SetHeader("Content-Length", strconv.Itoa(data.len()))?
    w.WriteHeader(200)?
    let _, err = w.Write(data[:])
    return err
}

func homepage(w mut http.ResponseWriter, r mut http.Request) error {
    return asset(w, "assets/index.html", "text/html; charset=utf-8")
}

func stylesheet(w mut http.ResponseWriter, r mut http.Request) error {
    return asset(w, "assets/site.css", "text/css; charset=utf-8")
}

func binary(w mut http.ResponseWriter, r mut http.Request) error {
    return asset(w, "assets/payload.bin", "application/octet-stream")
}

func health(w mut http.ResponseWriter, r mut http.Request) error {
    return w.Text(200, "ok")
}

func drain(w mut http.ResponseWriter, r mut http.Request) error {
    println("drain-handler-entered")
    var data = Array<byte>{0}
    let count, err = r.Read(data[:])
    if err != nil { return err }
    if count != 1 || data[0] != 120 { return error("unexpected drain payload") }
    return w.Text(200, "drained")
}

func run() error {
    var routes = http.NewRouterBuilder()
    routes.Handle("GET /", homepage)
    routes.Handle("GET /site.css", stylesheet)
    routes.Handle("GET /payload.bin", binary)
    routes.Handle("GET /healthz", health)
    routes.Handle("POST /drain", drain)
    let router = routes.Build()?
    var config = http.DefaultConfig()
    config.ReadBodyTimeout = 10 * time.Second
    config.IdleTimeout = 10 * time.Second
    config.AcceptPollInterval = 10 * time.Millisecond
    config.ShutdownTimeout = 2 * time.Second
    return http.ListenAndServeWithConfig("0.0.0.0:8080", router, config)
}

func main() {
    let err = run()
    if err != nil {
        println(errors.Message(err))
        os.Exit(1)
    }
}
'''

DOCKERFILE = '''ARG BUILDER_IMAGE
FROM ${BUILDER_IMAGE} AS build
WORKDIR /independent-project
COPY zore.toml main.ore ./
COPY assets ./assets
RUN zore install --offline . \\
    && cp zore.lock /tmp/zore.lock \\
    && zore install --locked --offline . \\
    && zore build --locked --offline main.ore \\
    && cmp /tmp/zore.lock zore.lock

FROM ubuntu:24.04
RUN apt-get update \\
    && apt-get install -y --no-install-recommends libgcc-s1 \\
    && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=build /independent-project/main /app/site
COPY --from=build /independent-project/assets /app/assets
USER 65532:65532
EXPOSE 8080
STOPSIGNAL SIGTERM
CMD ["/app/site"]
'''


def docker(*arguments, timeout=60):
    result = subprocess.run(
        ["docker", *arguments], capture_output=True, text=True,
        encoding="utf-8", errors="replace", timeout=timeout,
    )
    if result.returncode:
        raise RuntimeError(
            f"docker {' '.join(arguments)} failed ({result.returncode})\n"
            f"{result.stdout}{result.stderr}"
        )
    return result.stdout.strip()


def require(condition, message):
    if not condition:
        raise RuntimeError(message)


def inspect(name):
    return json.loads(docker("inspect", name))[0]


def wait_until(predicate, message, timeout=10):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if predicate():
            return
        time.sleep(0.025)
    raise RuntimeError(message)


def exchange(port, method, path, expected_status, expected_body=None, content_type=None):
    connection = http.client.HTTPConnection("127.0.0.1", port, timeout=5)
    try:
        connection.request(method, path)
        response = connection.getresponse()
        body = response.read()
        require(response.status == expected_status, f"{method} {path}: status {response.status}")
        if expected_body is not None:
            require(body == expected_body, f"{method} {path}: body differs")
        if content_type is not None:
            require(response.getheader("Content-Type") == content_type, f"{path}: content type differs")
        if method == "HEAD":
            require(body == b"", f"HEAD {path}: received a body")
        if method == "HEAD" and expected_status == 200:
            require(response.getheader("Content-Length") == str(len(BINARY)), "HEAD binary length differs")
    finally:
        connection.close()


def ready(port):
    try:
        exchange(port, "GET", "/healthz", 200, b"ok")
        return True
    except (OSError, http.client.HTTPException):
        return False


def verify_shutdown(container, port):
    idle = http.client.HTTPConnection("127.0.0.1", port, timeout=5)
    active = None
    try:
        idle.request("GET", "/healthz")
        response = idle.getresponse()
        require(response.status == 200 and response.read() == b"ok", "keepalive setup failed")
        require(idle.sock is not None, "server did not retain keepalive connection")
        idle_socket = idle.sock
        active = socket.create_connection(("127.0.0.1", port), timeout=5)
        active.sendall(b"POST /drain HTTP/1.1\r\nHost: localhost\r\nContent-Length: 1\r\nConnection: close\r\n\r\n")
        wait_until(
            lambda: "drain-handler-entered" in docker("logs", container),
            "active request never entered the handler",
        )
        started = time.monotonic()
        docker("kill", "--signal=SIGTERM", container)
        idle_socket.settimeout(1)
        try:
            require(idle_socket.recv(1) == b"", "idle connection survived SIGTERM")
        except ConnectionResetError:
            pass
        require(inspect(container)["State"]["Running"], "server exited before draining the active request")
        active.sendall(b"x")
        drained = http.client.HTTPResponse(active)
        drained.begin()
        require(drained.status == 200 and drained.read() == b"drained", "active request was not drained")
        require(docker("wait", container, timeout=6) == "0", "SIGTERM did not produce exit status zero")
        require(time.monotonic() - started < 6, "graceful shutdown exceeded its bound")
    finally:
        idle.close()
        if active is not None:
            active.close()


def smoke(image, version):
    require(
        docker("run", "--rm", image, "zore", "--version") == f"zore {version}",
        "builder Zore version differs from the release",
    )
    builder = inspect(image)
    require(builder["Architecture"] == "amd64" and builder["Os"] == "linux", "builder must target linux/amd64")
    require(builder["Config"].get("Entrypoint") is None, "builder entrypoint intercepts Docker RUN commands")
    require(builder["Config"]["Labels"].get("org.opencontainers.image.version") == version, "builder version label differs")
    suffix = uuid.uuid4().hex
    runtime_image = f"zore-builder-smoke:{suffix}"
    container = f"zore-builder-smoke-{suffix}"
    with tempfile.TemporaryDirectory(prefix="zore-builder-smoke-") as temporary:
        project = Path(temporary)
        (project / "assets").mkdir()
        (project / "zore.toml").write_text('name = "release_site"\n', encoding="utf-8")
        (project / "main.ore").write_text(SOURCE, encoding="utf-8")
        (project / "Dockerfile").write_text(DOCKERFILE, encoding="utf-8")
        for name, data in [("index.html", HOMEPAGE), ("site.css", STYLESHEET), ("payload.bin", BINARY)]:
            (project / "assets" / name).write_bytes(data)
        try:
            docker("build", "--build-arg", f"BUILDER_IMAGE={image}", "--tag", runtime_image, str(project), timeout=900)
            runtime = inspect(runtime_image)
            require(runtime["Config"]["User"] == "65532:65532", "runtime is not configured for the nonroot user")
            require(runtime["Config"]["Cmd"] == ["/app/site"], "runtime command is not the native executable")
            require(runtime["Config"].get("Entrypoint") is None, "runtime entrypoint intercepts signals")
            dependencies = docker("run", "--rm", "--entrypoint", "/usr/bin/ldd", runtime_image, "/app/site")
            require("not found" not in dependencies, f"missing runtime library: {dependencies}")
            docker(
                "run", "--rm", "--entrypoint", "/bin/sh", runtime_image, "-ec",
                'test "$(id -u)" = 65532; '
                'for tool in zore rustc rustup cargo clang cc gcc git; do '
                'if command -v "$tool"; then exit 1; fi; done; '
                'test ! -e /opt/rust; test ! -e /opt/cargo; '
                'test "$(find /app -type f | wc -l)" -eq 4',
            )
            docker("run", "--detach", "--name", container, "--publish", "127.0.0.1::8080", runtime_image)
            port = int(inspect(container)["NetworkSettings"]["Ports"]["8080/tcp"][0]["HostPort"])
            wait_until(lambda: ready(port), "runtime HTTP server did not become ready", timeout=20)
            exchange(port, "GET", "/", 200, HOMEPAGE, "text/html; charset=utf-8")
            exchange(port, "GET", "/site.css", 200, STYLESHEET, "text/css; charset=utf-8")
            exchange(port, "GET", "/payload.bin", 200, BINARY, "application/octet-stream")
            exchange(port, "HEAD", "/payload.bin", 200, b"", "application/octet-stream")
            exchange(port, "GET", "/missing", 404)
            exchange(port, "HEAD", "/missing", 404, b"")
            exchange(port, "GET", "/healthz", 200, b"ok")
            verify_shutdown(container, port)
        except Exception:
            subprocess.run(["docker", "logs", container], check=False, timeout=15)
            raise
        finally:
            subprocess.run(["docker", "rm", "--force", container], capture_output=True, check=False, timeout=30)
            subprocess.run(["docker", "image", "rm", "--force", runtime_image], capture_output=True, check=False, timeout=30)
    print(f"ok: {image} built and served an independent native application; SIGTERM drained it cleanly")


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--image", required=True)
    parser.add_argument("--version", required=True)
    arguments = parser.parse_args()
    smoke(arguments.image, arguments.version)


if __name__ == "__main__":
    main()
