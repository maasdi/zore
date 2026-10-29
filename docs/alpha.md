# Alpha compiler distribution

**Zore** is pronounced **"zor"**; its **`.ore`** source-file extension is
pronounced **"or"**. See the [official naming rules](../spec/language-spec.md#21-naming--locked).

The first alpha is `v0.1.0-alpha.1`. It distributes the supported compiler subset;
it does not declare full MVP conformance or stable language/API compatibility.
Known boundaries and executable evidence remain in the
[MVP inventory](conformance/mvp.md), [roadmap](roadmap.md) and
[platform guide](platform-support.md).

## Downloads and prerequisites

Use the exact version from [GitHub Releases](https://github.com/maasdi/zore/releases),
including prereleases. Download the archive for your host and its `.sha256` file.
Each archive contains `zore` or `zore.exe`, `LICENSE` and `artifact.json`.
The manifest records the version, target, source commit, Rust toolchain and
compiler-binary checksum. `SHA256SUMS` collects release asset checksums.

| Archive target | Host requirement |
| --- | --- |
| `x86_64-unknown-linux-gnu` | Linux x64 with glibc 2.39 or newer; built and tested on Ubuntu 24.04 |
| `aarch64-apple-darwin` | Apple Silicon macOS 15 or newer |
| `x86_64-apple-darwin` | Intel macOS 15 or newer |
| `x86_64-pc-windows-msvc` | Windows x64; native builds require Visual Studio C++ Build Tools and Windows SDK |

These are host builds, not cross-compilers. Other host versions, Linux musl,
Linux ARM and Windows ARM/GNU are not validated release artifacts.

The compiler executable includes bundled standard-package and runtime sources;
installation does not require cloning or compiling the compiler repository.
`check` and `fmt` work without a native build toolchain. `build`, `run` and
`test` still need Rust 1.98+ and clang LLVM 15+, plus the host linker and system
libraries. Rust compiles and links the embedded runtime. Use Rust 1.98.1 for
the tested alpha toolchain; set `ZORE_RUSTC` and `ZORE_CC` to select executables.

Linux example:

```sh
version=0.1.0-alpha.1
archive="zore-${version}-x86_64-unknown-linux-gnu.tar.gz"
base="https://github.com/maasdi/zore/releases/download/v${version}"
curl -fLO "$base/$archive"
curl -fLO "$base/$archive.sha256"
sha256sum -c "$archive.sha256"
mkdir -p "$HOME/.local/bin" zore-alpha
tar -xzf "$archive" -C zore-alpha
install -m 0755 zore-alpha/zore "$HOME/.local/bin/zore"
"$HOME/.local/bin/zore" --version
```

Add the installation directory to `PATH`. On macOS use the matching archive
and `shasum -a 256 -c`; on Windows compare `Get-FileHash -Algorithm SHA256`
with the downloaded checksum, expand the zip and add its directory to `PATH`.
Checksums verify transfer integrity; obtain both archive and checksum from the
trusted release. Preserve `artifact.json` with the download for provenance.

## Linux builder image

`ghcr.io/maasdi/zore-builder:0.1.0-alpha.1` contains the prebuilt Linux compiler,
Rust 1.98.1, clang and host build dependencies on Ubuntu 24.04, for `linux/amd64`.
It does not build the compiler from source. A project using only bundled
standard packages can build without a compiler-source checkout.

GHCR may initially require the package owner to make the package public.
Anonymous pulling must be verified before relying on it in another repository.
The release also carries the validated image as
`zore-builder-0.1.0-alpha.1-linux-amd64.tar.gz` with a checksum, so it can be
downloaded, verified and loaded with `docker load` without registry credentials:

```sh
sha256sum -c zore-builder-0.1.0-alpha.1-linux-amd64.tar.gz.sha256
gzip -dc zore-builder-0.1.0-alpha.1-linux-amd64.tar.gz | docker load
docker run --rm ghcr.io/maasdi/zore-builder:0.1.0-alpha.1 zore --version
```

Pin the exact image digest in consuming CI once published, rather than `latest`.
The builder has no special entrypoint and uses `/workspace` as its work directory.
The Rust version is explicitly selected even outside a Rust source checkout.

A separate application repository can use this pattern after generating and
committing its `zore.lock` with `zore install .`:

```dockerfile
FROM ghcr.io/maasdi/zore-builder:0.1.0-alpha.1 AS build
WORKDIR /project
COPY . .
RUN zore install --locked --offline . \
    && zore check --locked --offline app/main.ore \
    && zore test --locked --offline . \
    && zore build --locked --offline app/main.ore

FROM ubuntu:24.04
RUN apt-get update \
    && apt-get install -y --no-install-recommends libgcc-s1 \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=build /project/main /app/site
COPY --from=build /project/public /app/public
USER 65532:65532
EXPOSE 8080
STOPSIGNAL SIGTERM
CMD ["/app/site"]
```

This example assumes the entry package is `app/`, assets are in `public/`, and
the server binds `0.0.0.0:8080`. `build app/main.ore` writes `./main` in the
current directory. There is no `zore build --release` flag; native builds
already optimize generated code. External Git dependencies need their verified
cache populated before offline builds; see [dependency installation](git-dependencies.md).
Check native-library dependencies with `ldd` in the final image; do not assume
a static executable or an Alpine/scratch-compatible binary.

## Release process and validation

The release workflow validates archives on Linux x64, macOS ARM/Intel and
Windows x64. It extracts each archive outside the compiler checkout and checks
its displayed version, frontend independence and an independent native project.
The Linux builder check builds a separate HTTP application and a nonroot
runtime image, exercises HTML/CSS/binary responses, HEAD, 404 and health, and
requires an active request to finish during SIGTERM shutdown with exit status 0.
The final runtime is checked for missing shared libraries and accidental build
tools. These checks validate the packaging path, not arbitrary website programs.

Before tagging, require the release PR's ordinary CI and alpha-distribution
checks to pass. Merge to `main`, wait for ordinary CI on that exact commit,
then push the matching `v0.1.0-alpha.1` tag. The tag workflow verifies version,
main-branch ancestry and successful CI on the same commit, rebuilds and validates
the artifacts, publishes the versioned builder image and creates a draft
prerelease. Inspect assets/checksums, image provenance and installation evidence
before publishing the draft. Published tags/assets must not be overwritten;
fix a failed published version with a new alpha number.

The compiler bounds in project metadata remain numeric: `0.1.0-alpha.1` is
compared as `0.1.0` for `zore-min` and `zore-max`. Package release versions
still reject prerelease suffixes. This does not imply stable compatibility.

Website code, domain setup and Fly deployment remain in the independent website
repository. No Fly application, deployment token or production service is
created by this release workflow.
