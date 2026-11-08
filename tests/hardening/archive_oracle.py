"""Exercise archive I/O against Python tarfile and zipfile."""

import binascii
import gzip
import io
import os
from pathlib import Path
import random
import struct
import subprocess
import tarfile
import tempfile
import zipfile


PROGRAM = '''package main
import "std/archive"
import "std/compress/gzip"
import "std/errors"
import "std/io"
import "std/os"
import "std/strconv"

type entryWriter interface {
    mut WriteHeader(header archive.Header) error
    mut Write(data []byte) (int, error)
    mut Finish() error
}
type entryReader interface {
    mut Next() (archive.Header, bool, error)
    mut Read(buf mut []byte) (int, error)
}
func emit(writer mut entryWriter, data []byte, name string) error {
    writer.WriteHeader(archive.Header{Name: "dir/", Size: 0, Directory: true})?
    writer.WriteHeader(archive.Header{Name: name, Size: data.len(), Directory: false})?
    let count, err = writer.Write(data)
    if err != nil { return err }
    if count != data.len() { return error("short payload write") }
    writer.WriteHeader(archive.Header{Name: "empty", Size: 0, Directory: false})?
    writer.Finish()?
    return writer.Finish()
}
func drain(reader mut entryReader, sink mut io.Writer) error {
    for {
        let header, present, err = reader.Next()
        if err != nil { return err }
        if !present { return nil }
        println(header.Name)
        println(header.Size)
        println(header.Directory)
        let count, copyErr = io.Copy(sink, reader)
        if copyErr != nil { return copyErr }
        if count != header.Size { return error("entry size disagreement") }
    }
}
func run() error {
    let args = os.Args()
    let archiveLimit = strconv.Atoi(args[4])?
    let dataLimit = strconv.Atoi(args[5])?
    let entryLimit = strconv.Atoi(args[6])?
    if args[1] == "rt" || args[1] == "rz" || args[1] == "rtgz" {
        let source = os.Open(args[2])?
        var sink = os.Create(args[3])?
        if args[1] == "rtgz" {
            let compressed = gzip.NewReader(source, archiveLimit, archiveLimit)?
            var reader = archive.NewTARReader(compressed, archiveLimit, dataLimit, entryLimit)?
            drain(reader, sink)?
            return reader.Close()
        }
        if args[1] == "rt" {
            var reader = archive.NewTARReader(source, archiveLimit, dataLimit, entryLimit)?
            drain(reader, sink)?
            return reader.Close()
        }
        var reader = archive.NewZIPReader(source, archiveLimit, dataLimit, entryLimit)?
        drain(reader, sink)?
        return reader.Close()
    }
    let data = os.ReadFile(args[2])?
    let sink = os.Create(args[3])?
    if args[1] == "wtgz" {
        let compressed = gzip.NewWriter(sink, archiveLimit, archiveLimit)?
        var writer = archive.NewTARWriter(compressed, archiveLimit, dataLimit, entryLimit)?
        emit(writer, data[:], args[7])?
        let recovered, err = writer.Sink()
        if err != nil { return err }
        return recovered.Close()
    }
    if args[1] == "wt" || args[1] == "et" || args[1] == "wtdir" {
        var writer = archive.NewTARWriter(sink, archiveLimit, dataLimit, entryLimit)?
        if args[1] == "wt" { emit(writer, data[:], args[7])? }
        if args[1] == "wtdir" { writer.WriteHeader(archive.Header{Name: args[7], Size: 0, Directory: true})? }
        return writer.Close()
    }
    var writer = archive.NewZIPWriter(sink, archiveLimit, dataLimit, entryLimit)?
    if args[1] == "wz" { emit(writer, data[:], args[7])? }
    return writer.Close()
}
func main() {
    let err = run()
    if err != nil { panic(errors.Message(err)) }
}
'''


def tar_bytes(entries, archive_format=tarfile.USTAR_FORMAT):
    output = io.BytesIO()
    with tarfile.open(fileobj=output, mode='w', format=archive_format) as archive:
        for name, data in entries:
            header = tarfile.TarInfo(name)
            if name.endswith('/'):
                header.type = tarfile.DIRTYPE
            header.size = len(data)
            archive.addfile(header, io.BytesIO(data))
    return output.getvalue()


class Unseekable(io.BytesIO):
    def seekable(self):
        return False

    def seek(self, *args):
        raise io.UnsupportedOperation('unseekable')


def zip_bytes(entries, streamed=False, compression=zipfile.ZIP_STORED, comment=b''):
    output = Unseekable() if streamed else io.BytesIO()
    with zipfile.ZipFile(output, 'w', compression=compression) as archive:
        archive.comment = comment
        for name, data in entries:
            info = zipfile.ZipInfo(name)
            info.filename = name
            info.orig_filename = name
            info.compress_type = compression
            if name.endswith('/'):
                info.external_attr = (0o40775 << 16) | 16
            archive.writestr(info, data)
    return output.getvalue()


def repair_tar_checksum(data):
    result = bytearray(data)
    result[148:156] = b' ' * 8
    result[148:156] = f'{sum(result[:512]):06o}\0 '.encode()
    return bytes(result)


def crc_payload(target):
    baseline = binascii.crc32(b'\0' * 4)
    columns = []
    for bit in range(32):
        sample = bytearray(4)
        sample[bit // 8] = 1 << (bit % 8)
        columns.append(binascii.crc32(sample) ^ baseline)
    basis = {}
    for bit, column in enumerate(columns):
        value, mask = column, 1 << bit
        while value:
            pivot = value.bit_length() - 1
            if pivot not in basis:
                basis[pivot] = value, mask
                break
            value ^= basis[pivot][0]
            mask ^= basis[pivot][1]
    value, mask = target ^ baseline, 0
    while value:
        pivot = value.bit_length() - 1
        value ^= basis[pivot][0]
        mask ^= basis[pivot][1]
    payload = mask.to_bytes(4, 'little')
    assert binascii.crc32(payload) == target
    return payload


def unsigned_descriptor(data):
    descriptor = data.index(b'PK\x07\x08')
    result = bytearray(data[:descriptor] + data[descriptor + 4:])
    end = result.rindex(b'PK\x05\x06')
    central = struct.unpack_from('<I', result, end + 16)[0]
    struct.pack_into('<I', result, end + 16, central - 4)
    return bytes(result)


def main():
    root = Path(__file__).resolve().parents[2]
    compiler = root / 'target' / 'debug' / ('zore.exe' if os.name == 'nt' else 'zore')
    checked = 0
    rng = random.Random(224)
    with tempfile.TemporaryDirectory(prefix='zore-archives-') as temporary:
        directory = Path(temporary)
        source = directory / 'main.ore'
        source.write_text(PROGRAM, encoding='utf-8')
        built = subprocess.run([str(compiler), 'build', str(source)], cwd=directory,
                               capture_output=True, timeout=180)
        assert built.returncode == 0, built.stderr
        executable = directory / ('main.exe' if os.name == 'nt' else 'main')
        input_path, output_path = directory / 'input', directory / 'output'

        def run(mode, data, valid=True, archive_limit=None, data_limit=1048576,
                entry_limit=100, name='dir/é.txt', label=''):
            nonlocal checked
            input_path.write_bytes(data)
            result = subprocess.run(
                [str(executable), mode, str(input_path), str(output_path),
                 str(archive_limit if archive_limit is not None else 16777216),
                 str(data_limit), str(entry_limit), name],
                capture_output=True, timeout=30,
                env={**os.environ, 'ZORE_CHECK_LEAKS': '1'})
            checked += 1
            assert (result.returncode == 0) == valid, (mode, label or name, len(data), result.stderr)
            assert b'leak:' not in result.stderr, result.stderr
            headers = []
            if valid and mode.startswith('r'):
                lines = result.stdout.decode('utf-8').splitlines()
                assert len(lines) % 3 == 0, lines
                headers = [(lines[i], int(lines[i + 1]), lines[i + 2] == 'true')
                           for i in range(0, len(lines), 3)]
            return output_path.read_bytes(), headers

        for payload in [b'', b'hello archive\n', bytes(range(256)) * 256,
                        rng.randbytes(70000)]:
            entries = [('dir/', b''), ('dir/é.txt', payload), ('empty', b'')]
            expected = [(name, len(data), name.endswith('/')) for name, data in entries]
            for mode, encoded in [('rt', tar_bytes(entries)), ('rz', zip_bytes(entries)),
                                  ('rz', zip_bytes(entries, streamed=True))]:
                actual, headers = run(mode, encoded, archive_limit=len(encoded),
                                      data_limit=len(payload), entry_limit=3)
                assert actual == payload and headers == expected, (mode, headers)
                run(mode, encoded, valid=False, archive_limit=len(encoded) - 1)
                run(mode, encoded, valid=False, entry_limit=2)
                if payload:
                    run(mode, encoded, valid=False, data_limit=len(payload) - 1)
            encoded, _ = run('wt', payload, data_limit=len(payload), entry_limit=3)
            with tarfile.open(fileobj=io.BytesIO(encoded)) as archive:
                assert [member.name for member in archive] == ['dir', 'dir/é.txt', 'empty']
                assert archive.extractfile('dir/é.txt').read() == payload
            assert run('rt', encoded, archive_limit=len(encoded))[0] == payload
            compressed, _ = run('wtgz', payload, data_limit=len(payload), entry_limit=3)
            with tarfile.open(fileobj=io.BytesIO(compressed), mode='r:gz') as archive:
                assert archive.extractfile('dir/é.txt').read() == payload
            assert run('rtgz', compressed)[0] == payload
            assert run('rtgz', gzip.compress(tar_bytes(entries), mtime=0))[0] == payload
            corrupt = bytearray(compressed)
            corrupt[-8] ^= 1
            run('rtgz', corrupt, valid=False)
            run('rtgz', compressed[:-1], valid=False)
            encoded, _ = run('wz', payload, data_limit=len(payload), entry_limit=3)
            with zipfile.ZipFile(io.BytesIO(encoded)) as archive:
                assert archive.namelist() == [name for name, _ in entries]
                assert archive.read('dir/é.txt') == payload
                assert archive.testzip() is None
            assert run('rz', encoded, archive_limit=len(encoded))[0] == payload
            for mode in ['wt', 'wz']:
                run(mode, payload, valid=False, entry_limit=2)
                if payload:
                    run(mode, payload, valid=False, data_limit=len(payload) - 1)
            run('wz', payload, valid=False, archive_limit=len(encoded) - 1)


        for write_mode, read_mode, size in [('et', 'rt', 1024), ('ez', 'rz', 22)]:
            encoded, _ = run(write_mode, b'', archive_limit=size, data_limit=0, entry_limit=0)
            assert len(encoded) == size
            assert run(read_mode, encoded, archive_limit=size, data_limit=0, entry_limit=0) == (b'', [])
            run(write_mode, b'', valid=False, archive_limit=size - 1, data_limit=0, entry_limit=0)

        for name in ['../escape', '/absolute', 'a//b', './a', 'a/../b', 'a\\b',
                     'C:drive', 'nul.txt', 'COM1', 'lPt².log', 'a.', 'a ', 'a\nname', 'a\x85name', 'NUL .txt', 'a*', 'a?', 'a|b', 'a<b', 'a\"b']:
            for mode in ['rt', 'rz']:
                encoded = tar_bytes([(name, b'x')]) if mode == 'rt' else zip_bytes([(name, b'x')])
                run(mode, encoded, valid=False, label=repr(name))
            run('wt', b'x', valid=False, name=name)
            run('wz', b'x', valid=False, name=name)

        long_name = 'a' * 120 + '/' + 'b' * 95
        encoded, _ = run('wt', b'long', name=long_name)
        assert run('rt', encoded)[1][1][0] == long_name
        run('wt', b'', valid=False, name='x' * 101)
        long_directory = 'd' * 120 + '/'
        encoded, _ = run('wtdir', b'', name=long_directory)
        with tarfile.open(fileobj=io.BytesIO(encoded)) as archive:
            assert archive.getmembers()[0].name == long_directory.rstrip('/')
        assert run('rt', tar_bytes([(long_directory, b'')]))[1] == [(long_directory, 0, True)]

        for mode in ['rt', 'rz']:
            encoded = tar_bytes([('abc', b'payload')]) if mode == 'rt' else zip_bytes([('abc', b'payload')])
            lengths = list(range(0, min(80, len(encoded))))
            if mode == 'rt':
                lengths += [511, 512, 518, 519, 1023, 1024, 1535, 1536, 2047]
            else:
                lengths += list(range(max(0, len(encoded) - 30), len(encoded)))
            for length in sorted(set(lengths)):
                run(mode, encoded[:length], valid=False)

        tar = tar_bytes([('abc', b'payload')])
        for position in [0, 148, 1024, 1536]:
            corrupt = bytearray(tar)
            corrupt[position] ^= 1
            run('rt', corrupt, valid=False)
        corrupt = bytearray(tar)
        corrupt[519] = 1
        run('rt', corrupt, valid=False)
        for kind in [b'1', b'2', b'3', b'6', b'x', b'S']:
            corrupt = bytearray(tar)
            corrupt[156:157] = kind
            run('rt', repair_tar_checksum(corrupt), valid=False)
        corrupt = bytearray(tar)
        corrupt[124] = 128
        run('rt', repair_tar_checksum(corrupt), valid=False)
        for position in [100, 108, 116, 136, 329, 337]:
            corrupt = bytearray(tar)
            corrupt[position] = ord('9')
            run('rt', repair_tar_checksum(corrupt), valid=False)
        run('rt', tar + b'garbage', valid=False)
        assert run('rt', tar + b'\0' * 512)[0] == b'payload'

        for payload in [b'payload', crc_payload(0x08074b50)]:
            encoded = zip_bytes([('abc', payload)], streamed=True)
            unsigned = unsigned_descriptor(encoded)
            with zipfile.ZipFile(io.BytesIO(unsigned)) as archive:
                assert archive.read('abc') == payload
            assert run('rz', unsigned)[0] == payload
        encoded = zip_bytes([('abc', b'payload')])
        central = encoded.index(b'PK\x01\x02')
        end = encoded.rindex(b'PK\x05\x06')
        for position in [0, 6, 14, 18, 22, 30, 33, central, central + 8,
                         central + 16, central + 24, central + 42, end + 8, end + 12, end + 16]:
            corrupt = bytearray(encoded)
            corrupt[position] ^= 1
            run('rz', corrupt, valid=False)
        run('rz', encoded + b'garbage', valid=False)
        run('rz', b'prefix' + encoded, valid=False)
        run('rz', zip_bytes([('abc', b'payload')], compression=zipfile.ZIP_DEFLATED), valid=False)
        commented = zip_bytes([('abc', b'payload')], comment=b'bounded comment')
        assert run('rz', commented)[0] == b'payload'
        false_marker_comment = zip_bytes([('abc', b'payload')], comment=b'commentPK\x05\x06' + b'\0' * 18)
        assert run('rz', false_marker_comment)[0] == b'payload'
        for extra in [b'\x01\0\0\0', b'\xfe\xca\x03\0x', b'x']:
            output = io.BytesIO()
            with zipfile.ZipFile(output, 'w') as archive:
                info = zipfile.ZipInfo('abc')
                info.extra = extra
                archive.writestr(info, b'payload')
            run('rz', output.getvalue(), valid=False)
        output = io.BytesIO()
        with zipfile.ZipFile(output, 'w') as archive:
            info = zipfile.ZipInfo('link')
            info.create_system = 3
            info.external_attr = 0o120777 << 16
            archive.writestr(info, b'target')
        run('rz', output.getvalue(), valid=False)
        prefix = b'prefix'
        adjusted = bytearray(prefix + encoded)
        struct.pack_into('<I', adjusted, len(prefix) + end + 16, central + len(prefix))
        struct.pack_into('<I', adjusted, len(prefix) + central + 42, len(prefix))
        with zipfile.ZipFile(io.BytesIO(adjusted)) as archive:
            assert archive.read('abc') == b'payload'
        run('rz', adjusted, valid=False)
        oversized_directory = bytearray(16777217 + 22)
        struct.pack_into('<IHHHHIIH', oversized_directory, 16777217,
                         0x06054b50, 0, 0, 0, 0, 16777217, 0, 0)
        run('rz', oversized_directory, valid=False, archive_limit=len(oversized_directory))
        run('wz', b'', valid=False, name='x' * 1025)
        run('wt', b'', valid=False, name='')
        run('wz', b'', valid=False, name='')
        print(f'archive interoperability: {checked} cases passed')


if __name__ == '__main__':
    main()
