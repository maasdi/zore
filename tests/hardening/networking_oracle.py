"""Compare IP/URI results with independent references and exchange UDP packets."""

import errno
import ipaddress
import os
from pathlib import Path
import queue
import random
import socket
import struct
import subprocess
import tempfile
import threading
from urllib.parse import urlsplit


PROGRAM = r'''package main
import "std/net"
import "std/net/url"
import "std/encoding/binary"
import "std/errors"
import "std/os"
import "std/strconv"
import "std/strings"
import "std/time"

func convert(record string) (string, error) {
    let input = record[1:]
    if record[0] == 73 {
        let address, err = net.ParseIP(input)
        if err != nil { return "err", nil }
        return "ok:" + address.String(), nil
    }
    if record[0] == 65 {
        let address, err = net.ParseAddrPort(input)
        if err != nil { return "err", nil }
        return "ok:" + address.String(), nil
    }
    let left, right, _ = strings.Cut(input, "\n")
    if record[0] == 67 {
        let a = net.ParseIP(left)?
        let b = net.ParseIP(right)?
        return strconv.Itoa(a.Compare(b)), nil
    }
    let output, err = url.ResolveReference(left, right)
    if err != nil { return "err", nil }
    return "ok:" + output, nil
}
func run() error {
    let args = os.Args()
    if args[1] == "udp" {
        let endpoint = net.ParseAddrPort(args[2])?
        let conn = net.ListenUDP("udp", endpoint)?
        conn.SetDeadline(time.Now().Add(15 * time.Second))?
        println(conn.LocalAddr().String())
        var buffer = Array<byte>{0, 0, 0}
        for var i = 0; i < 6; i += 1 {
            let count, peer, truncated, err = conn.ReadFrom(buffer[:])
            if err != nil { return err }
            var reply = Array<byte>{0}
            if truncated { reply[0] = 1 }
            for var index = 0; index < count; index += 1 { reply.push(buffer[index]) }
            conn.WriteTo(reply[:], peer)?
        }
        return nil
    }
    let input = os.ReadFile(args[2])?
    var output = Array<byte>{}
    var offset = 0
    for offset < input.len() {
        let size = int(binary.LittleEndian.Uint32(input[offset:offset + 4])?)
        offset += 4
        let record = strings.FromBytes(input[offset:offset + size])?
        offset += size
        let converted = convert(record)?
        binary.LittleEndian.AppendUint32(output, uint32(converted.len()))
        for var index = 0; index < converted.len(); index += 1 { output.push(converted[index]) }
    }
    return os.WriteFile(args[3], output[:], 0o600)
}
func main() { let err = run(); if err != nil { panic(errors.Message(err)) } }
'''


def canonical(address):
    if isinstance(address, ipaddress.IPv6Address) and address.ipv4_mapped:
        return '::ffff:' + str(address.ipv4_mapped)
    return str(address)


def remove_dots(text):
    output = ''
    while text:
        if text.startswith('../'):
            text = text[3:]
        elif text.startswith('./'):
            text = text[2:]
        elif text.startswith('/./') or text == '/.':
            text = '/' + text[3:]
        elif text.startswith('/../') or text == '/..':
            text = '/' + text[4:]
            output = output.rsplit('/', 1)[0] if '/' in output else ''
        elif text in ('.', '..'):
            text = ''
        else:
            end = text.find('/', 1 if text.startswith('/') else 0)
            if end < 0:
                end = len(text)
            output += text[:end]
            text = text[end:]
    return output


def components(text):
    parsed = urlsplit(text)
    rest = text[len(parsed.scheme) + 1:] if parsed.scheme else text
    before_fragment, fragment_mark, fragment = text.partition('#')
    return dict(scheme=parsed.scheme, authority=parsed.netloc,
                has_authority=rest.startswith('//'), path=parsed.path,
                query=parsed.query, has_query='?' in before_fragment,
                fragment=fragment, has_fragment=bool(fragment_mark))


def resolve(base, reference):
    origin, result = components(base), components(reference)
    if result['scheme']:
        result['path'] = remove_dots(result['path'])
    else:
        result['scheme'] = origin['scheme']
        if result['has_authority']:
            result['path'] = remove_dots(result['path'])
        else:
            result['authority'] = origin['authority']
            result['has_authority'] = origin['has_authority']
            if not result['path']:
                result['path'] = origin['path']
                if not result['has_query']:
                    result['query'], result['has_query'] = origin['query'], origin['has_query']
            else:
                path = result['path']
                if not path.startswith('/'):
                    if origin['has_authority'] and not origin['path']:
                        path = '/' + path
                    else:
                        prefix = origin['path'].rsplit('/', 1)[0] + '/' if '/' in origin['path'] else ''
                        path = prefix + path
                result['path'] = remove_dots(path)
    output = result['scheme'] + ':'
    if result['has_authority']:
        output += '//' + result['authority']
    output += result['path']
    if result['has_query']:
        output += '?' + result['query']
    if result['has_fragment']:
        output += '#' + result['fragment']
    return output


def frame(records):
    output = bytearray()
    for text in records:
        value = text.encode('utf-8')
        output += struct.pack('<I', len(value)) + value
    return output


def unframe(data):
    records = []
    offset = 0
    while offset < len(data):
        size, = struct.unpack_from('<I', data, offset)
        offset += 4
        assert offset + size <= len(data)
        records.append(data[offset:offset + size].decode('utf-8'))
        offset += size
    return records


def udp_exchange(executable, family, environment):
    host, local = ('127.0.0.1', '127.0.0.1:0') if family == socket.AF_INET else ('::1', '[::1]:0')
    with socket.socket(family, socket.SOCK_DGRAM) as client:
        try:
            client.bind((host, 0))
        except OSError:
            if family == socket.AF_INET6:
                print('IPv6 loopback is not configured; IPv6 UDP exchange skipped')
                return False
            raise
        process = subprocess.Popen([str(executable), 'udp', local], stdout=subprocess.PIPE,
                                   stderr=subprocess.PIPE, env=environment)
        try:
            lines = queue.Queue()
            threading.Thread(target=lambda: lines.put(process.stdout.readline()), daemon=True).start()
            line = lines.get(timeout=15).decode('utf-8').strip()
            assert line, 'no UDP listener address'
            port = int(line.rsplit(':', 1)[1])
            client.settimeout(5)
            for data in [b'', b'a', b'abc', b'abcde', bytes([0, 255, 128]), b'x' * 65507]:
                try:
                    client.sendto(data, (host, port))
                except OSError as error:
                    if len(data) != 65507 or error.errno not in (errno.EMSGSIZE, 10040):
                        raise
                    print('Host UDP send limit rejects 65507 bytes; using a 4096-byte datagram')
                    data = b'x' * 4096
                    client.sendto(data, (host, port))
                response, peer = client.recvfrom(100)
                assert peer[1] == port, peer
                assert response == bytes([len(data) > 3]) + data[:3], (len(data), response)
            output, error = process.communicate(timeout=15)
            assert process.returncode == 0 and not output and not error, (output, error)
            return True
        finally:
            if process.poll() is None:
                process.kill()
                process.communicate(timeout=5)


def main():
    rng = random.Random(227)
    records, expected = [], []

    def add(record, result):
        records.append(record)
        expected.append(result)

    addresses = [ipaddress.ip_address(value) for value in ['0.0.0.0', '255.255.255.255', '::', '::1', '::ffff:192.0.2.1']]
    addresses.extend(ipaddress.IPv4Address(bytes([value, 0, 0, 255 - value])) for value in range(256))
    addresses.extend(ipaddress.IPv4Address(rng.getrandbits(32)) for _ in range(500))
    addresses.extend(ipaddress.IPv6Address(rng.getrandbits(128)) for _ in range(1000))
    for address in addresses:
        text = address.exploded.upper() if isinstance(address, ipaddress.IPv6Address) else str(address)
        add('I' + text, 'ok:' + canonical(address))
        port = rng.randrange(65536)
        endpoint = f'[{text}]:{port:05}' if address.version == 6 else f'{text}:{port:05}'
        output = f'[{canonical(address)}]:{port}' if address.version == 6 else f'{address}:{port}'
        add('A' + endpoint, 'ok:' + output)
    for _ in range(500):
        left, right = rng.choice(addresses), rng.choice(addresses)
        a, b = (left.version, left.packed), (right.version, right.packed)
        add('C' + str(left) + '\n' + str(right), str((a > b) - (a < b)))
    for text in ['', '127.1', '0177.0.0.1', '127.0.0.01', '0x7f000001', '1.2.3.256', '[::1]',
                 'fe80::1%3', '::/0', '1::2::3', ':::1', '1:2:3:4:5:6:7:8:9', '::ffff:01.2.3.4', 'é', '\0']:
        add('I' + text, 'err')
    for text in ['localhost:80', '::1:80', '[127.0.0.1]:80', '1.2.3.4:+1', '1.2.3.4:65536', '1.2.3.4:000080']:
        add('A' + text, 'err')
    bases = ['http://a/b/c/d;p?q', 'https://user@a:443/a//b/', 'file:///a/b', 'urn:a/b', 'http://[::1]:80', 'http://a?x#old']
    pieces = ['a', 'b', '.', '..', '', '%2e', '%2F', 'a;b', 'a@b', 'a:b']
    for _ in range(2000):
        base = rng.choice(bases)
        reference = '/'.join(rng.choice(pieces) for _ in range(rng.randrange(1, 10)))
        if reference.startswith('//'):
            reference = './' + reference
        if ':' in reference.split('/')[0] and not reference.startswith('a:b'):
            reference = './' + reference
        if rng.randrange(3) == 0:
            reference += '?' + rng.choice(['', 'x', 'a/../b', '?'])
        if rng.randrange(3) == 0:
            reference += '#' + rng.choice(['', 'f', 'a/../b', '?'])
        add('U' + base + '\n' + reference, 'ok:' + resolve(base, reference))
    for reference in ['//u@v@host/', '//[1:2]/', '//a:bad/', 'a b', 'é', '%', '%GG', 'x##y', 'a\\b', 'a\0b', '1:bad']:
        add('Uhttp://a/b\n' + reference, 'err')
    root = Path(__file__).resolve().parents[2]
    compiler = root / 'target' / 'debug' / ('zore.exe' if os.name == 'nt' else 'zore')
    environment = {**os.environ, 'ZORE_CHECK_LEAKS': '1'}
    with tempfile.TemporaryDirectory(prefix='zore-networking-') as temporary:
        directory = Path(temporary)
        source = directory / 'main.ore'
        source.write_text(PROGRAM, encoding='utf-8')
        built = subprocess.run([str(compiler), 'build', str(source)], cwd=directory, capture_output=True, timeout=180)
        assert built.returncode == 0, built.stderr
        executable = directory / ('main.exe' if os.name == 'nt' else 'main')
        input_path, output_path = directory / 'input', directory / 'output'
        input_path.write_bytes(frame(records))
        result = subprocess.run([str(executable), 'oracle', str(input_path), str(output_path)],
                                capture_output=True, timeout=30, env=environment)
        assert result.returncode == 0 and not result.stderr, result.stderr
        actual = unframe(output_path.read_bytes())
        assert len(actual) == len(expected)
        for index, (wanted, got) in enumerate(zip(expected, actual)):
            assert wanted == got, (index, records[index], wanted, got)
        udp_hosts = sum(udp_exchange(executable, family, environment) for family in [socket.AF_INET, socket.AF_INET6])
    print(f'Networking oracle: {len(records)} IP/endpoint/order/URI cases; UDP exchange on {udp_hosts} loopback families')


if __name__ == '__main__':
    main()
