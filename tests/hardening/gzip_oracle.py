"""Compare streaming gzip with Python's independent gzip/zlib implementation."""

import binascii
import gzip
import os
from pathlib import Path
import random
import subprocess
import tempfile
import zlib


PROGRAM = '''package main
import "std/compress/gzip"
import "std/errors"
import "std/io"
import "std/os"
import "std/strconv"

func run() error {
    let args = os.Args()
    let limit = strconv.Atoi(args[4])?
    let compressed = strconv.Atoi(args[5])?
    let source = os.Open(args[2])?
    var sink = os.Create(args[3])?
    if args[1] == "d" {
        var reader = gzip.NewReader(source, compressed, limit)?
        let _, err = io.Copy(sink, reader)
        if err != nil { return err }
        return reader.Close()
    }
    var writer = gzip.NewWriter(sink, limit, compressed)?
    var input = source
    let _, err = io.Copy(writer, input)
    if err != nil { return err }
    writer.Finish()?
    writer.Finish()?
    return writer.Close()
}

func main() {
    let err = run()
    if err != nil { panic(errors.Message(err)) }
}
'''


def pack_bits(parts):
    value = 0
    width = 0
    result = bytearray()
    for bits, count in parts:
        value |= bits << width
        width += count
        while width >= 8:
            result.append(value & 255)
            value >>= 8
            width -= 8
    if width:
        result.append(value)
    return bytes(result)


def empty_dynamic(oversubscribed=False, missing_end=False):
    parts = [(5, 3), (0, 5), (0, 5), (14, 4)]
    order = [16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1]
    parts.extend((int(symbol in (0, 1)), 3) for symbol in order)
    lengths = [0] * 258
    lengths[256] = int(not missing_end)
    if oversubscribed:
        lengths[0] = lengths[1] = 1
    parts.extend((length, 1) for length in lengths)
    parts.append((0, 1))
    return pack_bits(parts)


def main():
    root = Path(__file__).resolve().parents[2]
    compiler = root / 'target' / 'debug' / ('zore.exe' if os.name == 'nt' else 'zore')
    rng = random.Random(224)
    checked = 0
    with tempfile.TemporaryDirectory(prefix='zore-gzip-') as temporary:
        directory = Path(temporary)
        source = directory / 'main.ore'
        source.write_text(PROGRAM, encoding='utf-8')
        subprocess.run([str(compiler), 'build', str(source)], cwd=directory,
                       check=True, capture_output=True, timeout=180)
        executable = directory / ('main.exe' if os.name == 'nt' else 'main')
        input_path = directory / 'input'
        output_path = directory / 'output'

        def run(mode, data, limit, compressed=None, valid=True):
            nonlocal checked
            input_path.write_bytes(data)
            result = subprocess.run(
                [str(executable), mode, str(input_path), str(output_path),
                 str(limit), str(compressed if compressed is not None else 10485760)],
                capture_output=True, timeout=30, env={**os.environ, 'ZORE_CHECK_LEAKS': '1'})
            checked += 1
            assert (result.returncode == 0) == valid, (mode, len(data), result.stderr)
            assert b'leak:' not in result.stderr, result.stderr
            return output_path.read_bytes()

        payloads = [b'', b'a', b'hello gzip\n' * 100, b'\0' * 100000,
                    bytes(range(256)) * 300, rng.randbytes(70000)]
        payloads.extend(rng.randbytes(size) for size in [2, 3, 257, 258, 4095, 4096, 32767, 32768, 32769])
        payloads.extend((b'abcde' * 8000)[:size] for size in [257, 258, 4096, 32768, 32769])
        block_types = set()
        for payload in payloads:
            for level in [0, 1, 6, 9]:
                encoded = gzip.compress(payload, compresslevel=level, mtime=0)
                block_types.add((encoded[10] >> 1) & 3)
                assert run('d', encoded, len(payload), len(encoded)) == payload
            encoded = run('c', payload, len(payload))
            assert gzip.decompress(encoded) == payload
            assert run('d', encoded, len(payload), len(encoded)) == payload
            if payload:
                run('d', encoded, len(payload) - 1, valid=False)
                run('c', payload, len(payload) - 1, valid=False)

        assert block_types == {0, 1, 2}, block_types
        payload = b'optional headers and checksums' * 50
        compressor = zlib.compressobj(6, zlib.DEFLATED, -15)
        deflated = compressor.compress(payload) + compressor.flush()
        header = b'\x1f\x8b\x08\x1e\0\0\0\0\0\xff' + b'\x03\0abcname\0comment\0'
        crc = binascii.crc32(header) & 65535
        encoded = header + crc.to_bytes(2, 'little') + deflated
        encoded += binascii.crc32(payload).to_bytes(4, 'little') + len(payload).to_bytes(4, 'little')
        assert gzip.decompress(encoded) == payload
        assert run('d', encoded, len(payload), len(encoded)) == payload
        for position in [len(header), len(encoded) - 8, len(encoded) - 4]:
            corrupt = bytearray(encoded)
            corrupt[position] ^= 1
            run('d', corrupt, len(payload), valid=False)
        for length in range(len(encoded)):
            run('d', encoded[:length], len(payload), valid=False)
        run('d', encoded, len(payload), len(encoded) - 1, valid=False)
        run('c', b'', 0, 19, valid=False)
        assert len(run('c', b'', 0, 20)) == 20
        empty = gzip.compress(b'', mtime=0)
        assert run('d', empty * 1024, 0) == b''
        run('d', empty * 1025, 0, valid=False)
        joined = gzip.compress(b'first', mtime=0) + empty + gzip.compress(b'second', mtime=0)
        assert run('d', joined, 11, len(joined)) == b'firstsecond'
        run('d', joined, 10, valid=False)
        for suffix in [b'x', b'\0', b'\0' * 10, b'not a gzip header']:
            run('d', empty + suffix, 0, valid=False)
        for payload in [b'\x07', b'\x01\x01\0\0\0x', b'\x03\x02', b'\x05\0\0\0']:
            malformed = b'\x1f\x8b\x08\0\0\0\0\0\0\xff' + payload + b'\0' * 8
            run('d', malformed, 1000, valid=False)
        framing = b'\x1f\x8b\x08\0\0\0\0\0\0\xff'
        dynamic = framing + empty_dynamic() + b'\0' * 8
        assert gzip.decompress(dynamic) == b''
        assert run('d', dynamic, 0) == b''
        for invalid in [empty_dynamic(oversubscribed=True), empty_dynamic(missing_end=True),
                        pack_bits([(3, 3), (99, 8)]),
                        pack_bits([(3, 3), (64, 7), (15, 5)]),
                        pack_bits([(3, 3), (64, 7), (0, 5)])]:
            run('d', framing + invalid + b'\0' * 8, 1000, valid=False)
        for _ in range(100):
            noise = rng.randbytes(rng.randrange(1, 40))
            candidate = b'\x1f\x8b\x08\0\0\0\0\0\0\xff' + noise
            try:
                gzip.decompress(candidate)
            except (OSError, EOFError, zlib.error):
                run('d', candidate, 1048576, valid=False)
        print(f'gzip interoperability: {checked} cases passed')


if __name__ == '__main__':
    main()
