# IP addresses, DNS, UDP and URI references

`std/net` provides immutable IP/literal endpoint values, cancellable system name
lookup, TCP streams and UDP datagrams. `std/net/url` resolves URI references
separately from its strict HTTP URL parser. These APIs work on Linux, macOS and
Windows; their contracts are locked in spec §§37.36–37.39.

```ore
package main

import "std/net"
import "std/net/url"
import "std/strings"
import "std/time"
import "std/errors"

func run() error {
    let local = net.ParseAddrPort("127.0.0.1:0")?
    let server = net.ListenUDP("udp4", local)?
    let client = net.ListenUDP("udp4", local)?
    server.SetDeadline(time.Now().Add(5 * time.Second))?
    client.SetDeadline(time.Now().Add(5 * time.Second))?
    let message = strings.Bytes("hello")
    client.WriteTo(message[:], server.LocalAddr())?
    var buffer = Array<byte>{0, 0, 0, 0, 0}
    let count, peer, truncated, err = server.ReadFrom(buffer[:])
    if err != nil { return err }
    if truncated || !peer.IsValid() { return error("unexpected datagram") }
    println(strings.FromBytes(buffer[:count])?)
    println(url.ResolveReference("https://example.test/a/b", "../c?x#f")?)
    return nil
}

func main() {
    let err = run()
    if err != nil { panic(errors.Message(err)) }
}
```

## Address values and names

`ParseIP` accepts strict dotted IPv4 and IPv6 literals; it never looks up a name.
`ParseAddrPort` accepts `127.0.0.1:8080` or `[::1]:8080`. `NewAddrPort(ip, port)`
constructs an endpoint from a valid IP and an integer port in 0–65535; port 0
asks the OS for an available local port when binding. Endpoint input ports are
1–5 decimal digits, with leading zeroes normalized on output.

`IP` and `AddrPort` are Copy values with private fields. `IsValid` distinguishes
invalid results; failed constructors return invalid values and explicit errors.
Use `Equal` for equality and `Compare` for ordering. Invalid values sort first,
then IPv4 before IPv6, then address bytes; endpoints compare ports last. IPv4
and IPv4-mapped IPv6 stay distinct. `String` returns canonical lowercase literals
with IPv6 zero compression; endpoints bracket IPv6. Returned values outlive the
input string. `Addr`, `Port`, `Is4` and `Is6` expose the typed components.

Names, whitespace, octal/hexadecimal or abbreviated IPv4, brackets around a bare
IP, zone IDs, CIDR prefixes and IPvFuture are outside the IP parser's contract.

## Public name lookup

`net.LookupIP(ctx, "ip", "localhost")` uses the host's configured resolver.
`ip4` and `ip6` filter results. A valid literal bypasses the resolver and still
honors the requested family and cancellation. Ambiguous numeric forms such as
`127.1`, `0x7f000001` or `0x7f.0.0.1` fail instead of invoking legacy OS
numeric-address parsing.

The input is an ASCII hostname with normal label/length checks, optionally a
trailing dot; punycode/IDNA conversion belongs to the caller. The returned
`Array<net.IP>` owns its elements, removes duplicates, and preserves first
occurrence order from the system. Host files, local caches, search domains and
other resolver sources are platform policy. Ordering, system error details and
lookup latency vary by host. This API adds no DNS cache or TTL guarantee.

A cancelled or expired context returns an empty array and its context error.
Plain callers block with worker compensation; async callers suspend. Cancellation
stops waiting while an in-progress system resolver call finishes independently.
Its owned inputs and late result are released after completion. Abandoned queued
or running work can retain capacity in the existing bounded helper pool until
the resolver returns; the API cannot forcibly abort a platform resolver call.
Reverse/record-specific lookups, custom DNS servers, wire messages and DNSSEC
remain outside this increment.

## Datagram ownership, counts and deadlines

`ListenUDP` accepts `udp`, `udp4` or `udp6` and a literal `AddrPort`. A
family-specific network must match the local address; `udp` uses the literal's
family. `UDPConn` is Move: transfer ownership to a task or borrow it for an
operation. Dropping it closes the socket; `Close` consumes it. `LocalAddr`
returns an invalid endpoint for a closed handle. It is separate from stream
`Conn` and does not implement `io.Reader` or `io.Writer`.

`ReadFrom(buf)` returns `(copied, sender, truncated, error)` and consumes exactly
one packet. If the packet is larger than `buf`, it copies the prefix and discards
the tail. A zero-capacity buffer consumes a packet too. An empty datagram returns
zero bytes, a valid sender and no truncation/error; it is not EOF. Original
payload size beyond the copied count is not exposed. The implementation receives
a full ordinary datagram before copying, so Windows reports truncation reliably
too. Scoped peers are unsupported and cause an error after packet consumption.

`WriteTo(data, peer)` sends one packet to a literal endpoint of the bound family
with nonzero port. It supports empty payloads and at most 65507 bytes; the OS may
reject smaller payloads because of path MTU or other conditions. Success reports
the whole length; an OS short write reports its count and an error, without
sending the tail as another datagram. UDP delivery, ordering and uniqueness are
not guaranteed. IPv6 dual-stack behavior is platform-defined.

`SetDeadline`, `SetReadDeadline`, `SetWriteDeadline` and `ClearDeadline` work like
TCP deadlines. A past deadline fails before I/O, including empty writes, and a
timed-out read leaves queued packets untouched. Deadlines apply to waits started
after the setting. Async packet waits use the existing socket reactor on all
three hosts; plain waits allow replacement workers. Buffers stay borrowed until
the call completes, including suspension.

Connected UDP, multicast/broadcast configuration, ancillary data, explicit
interface/zone selection and jumbograms remain excluded. HTTP/2 and QUIC are
independent future protocols.

## Resolve a reference, then validate its destination

`url.ResolveReference(base, reference)` follows RFC 3986 merge and dot-segment
rules. It supports absolute and relative references, authority/root/path changes,
empty/query/fragment references and non-HTTP schemes. The base needs a scheme;
its fragment is ignored. Explicit empty `?` and `#` delimiters stay present.
Repeated slashes, percent escapes and encoded dot/slash segments are preserved;
only literal `.` and `..` segments are removed where RFC resolution requires it.
Inputs are ASCII URI syntax, with valid percent escapes and no controls,
whitespace or backslashes. Output is an independent immutable string.

Generic URI syntax allows user information, IPvFuture, empty authorities and
ports beyond the transport range. It does not validate DNS names or enforce an
HTTP redirect policy. Pass an HTTP result through `url.ParseAbsolute` and the
application's origin/scheme policy before using it. Existing `ParseAbsolute`,
`ParseTarget` and HTTP admission rules keep their existing restrictions.

## Implementation increments and evidence

Issue [#227](https://github.com/maasdi/zore/issues/227) is split into four focused
increments: IP/literal endpoint values, cancellable system lookup, UDP resources,
and URI resolution. IP validation is shared with URL parsing; HTTP host/port
policy remains in the URL package. No new dependency or Windows reactor is used.

The [conformance map](../tests/conformance/networking.md) names frontend, native,
runtime and independent oracle checks. DNS tests use host-local fixtures and a
held resolver gate, without requiring public internet access. UDP interoperability
uses Python peers on both loopback families; IPv6 exchange is skipped explicitly
when the host has no IPv6 loopback configuration.
