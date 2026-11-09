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
import zlib


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
    mut Next() (archive.Entry, bool, error)
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
        let entry, present, err = reader.Next()
        if err != nil { return err }
        if !present { return nil }
        if entry.Kind == archive.SymlinkEntry || entry.Kind == archive.HardLinkEntry { return error("link entry") }
        let header = entry.Header
        println(header.Name)
        println(header.Size)
        println(header.Directory)
        let count, copyErr = io.Copy(sink, reader)
        if copyErr != nil { return copyErr }
        if count != header.Size { return error("entry size disagreement") }
    }
}
func kindName(kind archive.EntryKind) string {
    if kind == archive.DirectoryEntry { return "dir" }
    if kind == archive.SymlinkEntry { return "symlink" }
    if kind == archive.HardLinkEntry { return "hardlink" }
    return "file"
}
func listEntries(reader mut entryReader, sink mut io.Writer) error {
    for {
        let entry, present, err = reader.Next()
        if err != nil { return err }
        if !present { return nil }
        println(entry.Header.Name + "|" + kindName(entry.Kind) + "|" + entry.Link + "|" + strconv.Itoa(entry.Header.Size) + "|" + strconv.FormatBool(entry.Executable))
        let _, copyErr = io.Copy(sink, reader)
        if copyErr != nil { return copyErr }
    }
}
func run() error {
    let args = os.Args()
    let archiveLimit = strconv.Atoi(args[4])?
    let dataLimit = strconv.Atoi(args[5])?
    let entryLimit = strconv.Atoi(args[6])?
    if args[1] == "lt" || args[1] == "lz" {
        let source = os.Open(args[2])?
        var sink = os.Create(args[3])?
        if args[1] == "lt" {
            var reader = archive.NewTARReader(source, archiveLimit, dataLimit, entryLimit)?
            listEntries(reader, sink)?
            return reader.Close()
        }
        var reader = archive.NewZIPReader(source, archiveLimit, dataLimit, entryLimit)?
        listEntries(reader, sink)?
        return reader.Close()
    }
    if args[1] == "wxt" || args[1] == "wxz" {
        let data = os.ReadFile(args[2])?
        let sink = os.Create(args[3])?
        let tool = archive.Entry{Header: archive.Header{Name: args[7], Size: data.len(), Directory: false}, Kind: archive.FileEntry, Link: "", Executable: true}
        if args[1] == "wxt" {
            var writer = archive.NewTARWriter(sink, archiveLimit, dataLimit, entryLimit)?
            writer.WriteEntry(tool)?
            let _, err = writer.Write(data[:])
            if err != nil { return err }
            return writer.Close()
        }
        var writer = archive.NewZIPWriter(sink, archiveLimit, dataLimit, entryLimit)?
        writer.WriteEntryWith(tool, archive.Deflate)?
        let _, err = writer.Write(data[:])
        if err != nil { return err }
        return writer.Close()
    }
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
    if args[1] == "wzd" {
        writer.WriteHeaderWith(archive.Header{Name: "dir/", Size: 0, Directory: true}, archive.Deflate)?
        writer.WriteHeaderWith(archive.Header{Name: args[7], Size: data.len(), Directory: false}, archive.Deflate)?
        let count, err = writer.Write(data[:]); if err != nil { return err }
        if count != data.len() { return error("short deflated write") }
        writer.WriteHeaderWith(archive.Header{Name: "empty", Size: 0, Directory: false}, archive.Deflate)?
    }
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


def tar_member(name, kind, data=b'', magic=b'ustar\x0000', size=None):
    block = bytearray(512)
    raw = name.encode('utf-8') if isinstance(name, str) else name
    block[0:len(raw)] = raw
    block[100:108] = b'0000644\0'
    block[108:116] = b'0000000\0'
    block[116:124] = b'0000000\0'
    block[124:136] = b'%011o\0' % (len(data) if size is None else size)
    block[136:148] = b'00000000000\0'
    block[156:157] = kind
    block[257:265] = magic
    block[148:156] = b' ' * 8
    block[148:156] = b'%06o\0 ' % sum(block)
    return bytes(block) + data + b'\0' * ((-len(data)) % 512)


def pax_payload(records):
    body = b''
    for key, value in records:
        text = b' ' + key.encode() + b'=' + value.encode() + b'\n'
        length = len(text) + 1
        while len(str(length)) + len(text) != length:
            length += 1
        body += str(length).encode() + text
    return body


TAR_END = b'\0' * 1024


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


def raw_zip(payload, raw, flags=0, declared=None):
    size = len(payload) if declared is None else declared
    crc = binascii.crc32(payload)
    local = struct.pack('<IHHHHHIIIHH', 0x04034b50, 20, flags, 8, 0, 33, crc, len(raw), size, 3, 0) + b'abc' + raw
    central = struct.pack('<IHHHHHHIIIHHHHHII', 0x02014b50, 20, 20, flags, 8, 0, 33, crc, len(raw), size, 3, 0, 0, 0, 0, 0, 0) + b'abc'
    end = struct.pack('<IHHHHIIH', 0x06054b50, 0, 0, 1, 1, len(central), len(local), 0)
    return local + central + end


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
            if valid and mode.startswith('l'):
                headers = [tuple(line.split('|')) for line in result.stdout.decode('utf-8').splitlines()]
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
                                  ('rz', zip_bytes(entries, streamed=True)),
                                  ('rz', zip_bytes(entries, compression=zipfile.ZIP_DEFLATED)),
                                  ('rz', zip_bytes(entries, streamed=True, compression=zipfile.ZIP_DEFLATED))]:
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
            deflated, _ = run('wzd', payload, data_limit=len(payload), entry_limit=3)
            with zipfile.ZipFile(io.BytesIO(deflated)) as archive:
                assert all(member.compress_type == zipfile.ZIP_DEFLATED for member in archive.infolist())
                assert archive.read('dir/é.txt') == payload
                assert archive.testzip() is None
            assert run('rz', deflated, archive_limit=len(deflated))[0] == payload
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

        for name in ['../escape', '/absolute', 'a//b', 'a/./b', 'a/../b', 'a\\b',
                     'C:drive', 'nul.txt', 'COM1', 'lPt².log', 'a.', 'a ', 'a\nname', 'a\x85name', 'NUL .txt', 'a*', 'a?', 'a|b', 'a<b', 'a\"b']:
            for mode in ['rt', 'rz']:
                encoded = tar_bytes([(name, b'x')]) if mode == 'rt' else zip_bytes([(name, b'x')])
                run(mode, encoded, valid=False, label=repr(name))
            run('wt', b'x', valid=False, name=name)
            run('wz', b'x', valid=False, name=name)

        long_name = 'a' * 120 + '/' + 'b' * 95
        encoded, _ = run('wt', b'long', name=long_name)
        assert run('rt', encoded)[1][1][0] == long_name
        for name in ['x' * 101, 'deep/' + 'é' * 300 + '.txt', 'y' * 1024]:
            encoded, _ = run('wt', b'pax', name=name)
            with tarfile.open(fileobj=io.BytesIO(encoded)) as archive:
                assert archive.getnames() == ['dir', name, 'empty'], archive.getnames()
                assert archive.extractfile(name).read() == b'pax'
            assert run('rt', encoded)[1][1] == (name, 3, False)
        long_tree = 'z' * 300 + '/'
        encoded, _ = run('wtdir', b'', name=long_tree)
        with tarfile.open(fileobj=io.BytesIO(encoded)) as archive:
            assert archive.getmembers()[0].name == long_tree.rstrip('/')
            assert archive.getmembers()[0].isdir()
        assert run('rt', encoded)[1] == [(long_tree, 0, True)]
        for name in ['./a', 'a/./b', '../a']:
            run('wt', b'x', valid=False, name=name)
            run('rz', zip_bytes([(name, b'x')]), valid=False, label=repr(name))

        long_path = 'deep/' + 'segment-é/' * 40 + 'file.txt'
        extended_entries = [('dir/', b''), (long_path, b'extended'), ('naïve/ü.txt', b'u'),
                            ('d/' + 'e' * 150 + '/', b'')]
        expected = [(name, len(data), name.endswith('/')) for name, data in extended_entries]
        for archive_format in [tarfile.PAX_FORMAT, tarfile.GNU_FORMAT]:
            output = io.BytesIO()
            with tarfile.open(fileobj=output, mode='w', format=archive_format) as archive:
                for name, data in extended_entries:
                    info = tarfile.TarInfo(name.rstrip('/'))
                    if name.endswith('/'):
                        info.type = tarfile.DIRTYPE
                    info.size = len(data)
                    info.mtime = 1700000000.5
                    info.uname = 'ada'
                    info.pax_headers = {'comment': 'ignored', 'SCHILY.xattr.user.k': 'v'}
                    archive.addfile(info, io.BytesIO(data))
            encoded = output.getvalue()
            actual, headers = run('rt', encoded, entry_limit=4)
            assert headers == expected, headers
            assert actual == b'extendedu', actual
            run('rt', encoded, valid=False, entry_limit=3)
        dot = tar_member('./', b'5') + tar_member('./a.txt', b'0', b'dot') + TAR_END
        assert run('rt', dot) == (b'dot', [('a.txt', 3, False)])
        gnu = b'ustar  \0'
        gnu_long = (tar_member('././@LongLink', b'L', b'x' * 200 + b'/name\0', magic=gnu)
                    + tar_member('short', b'0', b'gnu', magic=gnu) + TAR_END)
        assert run('rt', gnu_long)[1] == [('x' * 200 + '/name', 3, False)]
        both = (tar_member('././@LongLink', b'L', b'from-gnu\0', magic=gnu)
                + tar_member('PaxHeader', b'x', pax_payload([('path', 'from-pax')]))
                + tar_member('ustar-name', b'0', b'abc') + TAR_END)
        assert run('rt', both)[1] == [('from-pax', 3, False)]
        sized = (tar_member('PaxHeader', b'x', pax_payload([('size', '7')]))
                 + tar_member('sized', b'0', b'payload', size=0)[:512] + b'payload' + b'\0' * 505 + TAR_END)
        assert run('rt', sized) == (b'payload', [('sized', 7, False)])
        cleared = (tar_member('PaxHeader', b'x', pax_payload([('path', '')]))
                   + tar_member('kept', b'0', b'k') + TAR_END)
        assert run('rt', cleared)[1] == [('kept', 1, False)]
        global_ok = (tar_member('GlobalHead', b'g', pax_payload([('comment', 'all'), ('mtime', '1')]))
                     + tar_member('g.txt', b'0', b'g') + TAR_END)
        assert run('rt', global_ok)[1] == [('g.txt', 1, False)]
        file_entry = tar_member('file', b'0', b'f')
        for invalid in [
                pax_payload([('linkpath', 'target')]),
                pax_payload([('GNU.sparse.size', '10')]),
                pax_payload([('hdrcharset', 'BINARY')]),
                pax_payload([('size', '-1')]),
                pax_payload([('size', '1x')]),
                pax_payload([('size', str(1073741825))]),
                pax_payload([('path', '../escape')]),
                b'12 path=a\n',
                b'08 a=b\n',
                b'7 pathx\n',
                b'6 =ab\n',
                b'8 path=a',
                b'path=a\n',
                b'\xff']:
            run('rt', tar_member('PaxHeader', b'x', invalid) + file_entry + TAR_END, valid=False, label=repr(invalid))
        for invalid in [pax_payload([('path', 'all')]), pax_payload([('size', '1')])]:
            run('rt', tar_member('GlobalHead', b'g', invalid) + file_entry + TAR_END, valid=False)
        pax_header = tar_member('PaxHeader', b'x', pax_payload([('path', 'p')]))
        long_link = tar_member('././@LongLink', b'L', b'name\0', magic=gnu)
        for invalid in [pax_header + pax_header + file_entry + TAR_END,
                        long_link + long_link + file_entry + TAR_END,
                        pax_header + TAR_END,
                        long_link + TAR_END,
                        tar_member('PaxHeader', b'x', b'x' * 65537) + file_entry + TAR_END,
                        tar_member('././@LongLink', b'L', b'n' * 1026, magic=gnu) + file_entry + TAR_END,
                        tar_member('././@LongLink', b'K', b'target\0', magic=gnu) + file_entry + TAR_END,
                        tar_member('sparse', b'S', b'', magic=gnu) + TAR_END,
                        tar_member('PaxHeader', b'x', pax_payload([('path', 'p')]) + b'\x01') + file_entry + TAR_END]:
            run('rt', invalid, valid=False)
        def tar_info(name, kind=tarfile.REGTYPE, link='', data=b'', mode=0o644):
            info = tarfile.TarInfo(name)
            info.type, info.linkname, info.size, info.mode = kind, link, len(data), mode
            return info, io.BytesIO(data)

        long_target = 'x/' * 200 + 'target'
        for archive_format in [tarfile.PAX_FORMAT, tarfile.GNU_FORMAT, tarfile.USTAR_FORMAT]:
            output = io.BytesIO()
            with tarfile.open(fileobj=output, mode='w', format=archive_format) as archive:
                archive.addfile(*tar_info('bin/tool', data=b'#!', mode=0o755))
                archive.addfile(*tar_info('group-x', data=b'g', mode=0o610))
                archive.addfile(*tar_info('plain', data=b'p'))
                archive.addfile(*tar_info('soft', tarfile.SYMTYPE, 'plain'))
                archive.addfile(*tar_info('hard', tarfile.LNKTYPE, 'plain'))
                if archive_format != tarfile.USTAR_FORMAT:
                    archive.addfile(*tar_info('far', tarfile.SYMTYPE, long_target))
            encoded = output.getvalue()
            expected = [('bin/tool', 'file', '', '2', 'true'), ('group-x', 'file', '', '1', 'true'),
                        ('plain', 'file', '', '1', 'false'), ('soft', 'symlink', 'plain', '0', 'false'),
                        ('hard', 'hardlink', 'plain', '0', 'false')]
            if archive_format != tarfile.USTAR_FORMAT:
                expected.append(('far', 'symlink', long_target, '0', 'false'))
            assert run('lt', encoded)[1] == expected, run('lt', encoded)[1]
        output = io.BytesIO()
        with zipfile.ZipFile(output, 'w') as archive:
            for name, system, mode, data, compression in [
                    ('tool', 3, 0o100755, b'#!', zipfile.ZIP_STORED),
                    ('dos', 0, 0o100755, b'd', zipfile.ZIP_STORED),
                    ('soft', 3, 0o120777, b'tool', zipfile.ZIP_STORED),
                    ('deflated-link', 3, 0o120777, long_target.encode(), zipfile.ZIP_DEFLATED)]:
                info = zipfile.ZipInfo(name)
                info.create_system, info.external_attr, info.compress_type = system, mode << 16, compression
                archive.writestr(info, data)
        encoded = output.getvalue()
        assert run('lz', encoded)[1] == [('tool', 'file', '', '2', 'true'), ('dos', 'file', '', '1', 'false'),
                                         ('soft', 'symlink', 'tool', '0', 'false'),
                                         ('deflated-link', 'symlink', long_target, '0', 'false')]
        for data in [b'', b'x' * 1025, b'\xff', b'a\0b']:
            output = io.BytesIO()
            with zipfile.ZipFile(output, 'w') as archive:
                info = zipfile.ZipInfo('bad')
                info.create_system, info.external_attr = 3, 0o120777 << 16
                archive.writestr(info, data)
            run('lz', output.getvalue(), valid=False, label=repr(data[:4]))
        for invalid in [tar_member('soft', b'2', b'payload') + TAR_END,
                        tar_member('soft', b'2') + TAR_END,
                        tar_member('PaxHeader', b'g', pax_payload([('linkpath', 'x')])) + file_entry + TAR_END,
                        tar_member('PaxHeader', b'x', pax_payload([('linkpath', 'x')])) + file_entry + TAR_END,
                        tar_member('././@LongLink', b'K', b'x\0', magic=gnu) + file_entry + TAR_END,
                        tar_member('././@LongLink', b'K', b'x\0', magic=gnu) + TAR_END]:
            run('lt', invalid, valid=False)
        for mode, reader in [('wxt', 'lt'), ('wxz', 'lz')]:
            encoded, _ = run(mode, b'#!run', name='bin/run')
            assert run(reader, encoded)[1] == [('bin/run', 'file', '', '5', 'true')]
            if mode == 'wxt':
                with tarfile.open(fileobj=io.BytesIO(encoded)) as archive:
                    assert archive.getmember('bin/run').mode == 0o755
            else:
                with zipfile.ZipFile(io.BytesIO(encoded)) as archive:
                    info = archive.getinfo('bin/run')
                    assert info.create_system == 3 and info.external_attr >> 16 == 0o100755
                    assert archive.read('bin/run') == b'#!run'

        counted = pax_header + file_entry + TAR_END
        assert run('rt', counted, archive_limit=len(counted))[1] == [('p', 1, False)]
        run('rt', counted, valid=False, archive_limit=len(counted) - 1)
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
        assert run('rz', zip_bytes([('abc', b'payload')], compression=zipfile.ZIP_DEFLATED))[0] == b'payload'
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
        for payload in [b'', b'hello deflate', b'a' * 100000, rng.randbytes(40000)]:
            for level, strategy in [(0, zlib.Z_DEFAULT_STRATEGY), (6, zlib.Z_FIXED), (9, zlib.Z_DEFAULT_STRATEGY)]:
                compressor = zlib.compressobj(level, zlib.DEFLATED, -15, strategy=strategy)
                raw = compressor.compress(payload) + compressor.flush()
                encoded = raw_zip(payload, raw)
                assert run('rz', encoded)[0] == payload
                for hints in [2, 4, 6]:
                    assert run('rz', raw_zip(payload, raw, flags=hints))[0] == payload
                run('rz', raw_zip(payload, raw + b'x'), valid=False)
                run('rz', raw_zip(payload, raw[:-1]), valid=False)
                if payload:
                    run('rz', raw_zip(payload, raw, declared=len(payload) - 1), valid=False)
                    run('rz', raw_zip(payload, raw, declared=len(payload) + 1), valid=False)
                    run('rz', encoded, valid=False, data_limit=len(payload) - 1)
            streamed = zip_bytes([('abc', payload)], streamed=True, compression=zipfile.ZIP_DEFLATED)
            assert run('rz', unsigned_descriptor(streamed))[0] == payload
        for raw in [b'', b'\x07', b'\x03', b'\x01\x01\x00\x00\x00x']:
            run('rz', raw_zip(b'', raw), valid=False)
        run('rz', raw_zip(b'', b'\x03\x00', flags=1), valid=False)
        mismatch = bytearray(raw_zip(b'', b'\x03\x00'))
        struct.pack_into('<H', mismatch, 8, 0)
        run('rz', mismatch, valid=False)
        print(f'archive interoperability: {checked} cases passed')


if __name__ == '__main__':
    main()
