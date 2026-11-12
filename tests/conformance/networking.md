# Public networking conformance

Issue #227; locked spec §§37.36–37.39. All four increments retain the existing
ownership/error/async model and support Linux, macOS and Windows.

| Increment | Executable assertions | Exclusions |
| --- | --- | --- |
| IP and literal endpoint values | `tests/codegen/networking.rs` checks canonical literals, mapped-family distinctions, numeric ordering, invalid values, constructor errors and independent lifetimes. `tests/hardening/networking_oracle.py` compares strict parsing, formatting and ordering with Python `ipaddress`. | Hostname resolution in literal parsing, legacy numeric addresses, CIDR, zones and IP-parser IPvFuture |
| System name lookup | Native literal/family/name rejection, local `localhost` results, duplicate removal, caller ownership, precancellation, expired contexts, cancellation before a held resolver finishes, and late cleanup under a controlled one-worker runtime. | Reverse/raw/record-specific DNS, custom servers, DNSSEC, new caching/TTL guarantees or interrupting an OS resolver |
| UDP datagrams | Native binary/empty/truncated/zero-capacity packets, sender/count reporting, deadline preservation, size/family/peer errors, Move cleanup on normal/error/panic exits, plain worker compensation and async single-worker progress. `runtime/src/net_udp_tests.rs` forces a pending whole-datagram write, checks repeated pending-read polling, and preserves queued packets at expired deadlines. Python peers test IPv4 and configured IPv6 loopback, including a 65507-byte packet. | Connected UDP, multicast/broadcast controls, ancillary data, interface/zone selection and jumbograms |
| URI references | Native RFC 3986 normal/abnormal vectors, empty delimiters, encoded segments, invalid bytes and unchanged HTTP admission. The oracle uses Python URI splitting plus the RFC string algorithm, independently of the Zore segment stack, for seeded reference cases. | IRI/IDNA conversion, percent decoding, DNS validation and HTTP redirect/same-origin authorization |
| Frontend and visibility | `tests/packages/networking.rs` accepts the API without LLVM/native linking and rejects wrong argument types, private fields and use after consuming UDP close. | No new syntax, struct equality operators or runtime provider |
| User example | Compiled first example from `docs/networking.md`, checking exact output and leak-free cleanup. | Evidence covers these examples, not every network configuration |

Run after building the compiler:

```sh
cargo test --locked --test packages networking
cargo test --locked --test native networking
cargo test --locked -p zore-runtime udp_
python tests/hardening/networking_oracle.py
```

Native programs use `ZORE_CHECK_LEAKS=1` and bounded process execution. The DNS
gate is test-only, holds actual helper work before its resolver call, and releases
it after cancellation has returned to the caller; late tasks then quiesce. Public
internet access is not a precondition. IPv6 peer tests report an explicit skip if
IPv6 loopback is not configured. Python parsing/reference checks are bounded
conformance evidence, not a universal URI or network-protocol proof.
