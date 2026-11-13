"""Check confined extraction, rollback, and independent archive interoperability."""

import gzip
import io
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tarfile
import tempfile
import warnings
import zipfile

from archive_oracle import raw_zip, tar_bytes, zip_bytes
import zlib

PROGRAM = '''package main
import "std/archive"
import "std/compress/gzip"
import "std/errors"
import "std/os"
import "std/strconv"
func run() error {
    let args = os.Args()
    let archiveLimit = strconv.Atoi(args[4])?
    let dataLimit = strconv.Atoi(args[5])?
    let entryLimit = strconv.Atoi(args[6])?
    let source = os.Open(args[2])?
    if args[1] == "tarmerge" || args[1] == "zipmerge" {
        var options = archive.DefaultExtractOptions(archiveLimit, dataLimit, entryLimit)
        options.Merge = true
        if args[1] == "zipmerge" { return archive.ExtractZIPWith(source, args[3], options) }
        return archive.ExtractTARWith(source, args[3], options)
    }
    if args[1] == "tarlinks" || args[1] == "ziplinks" {
        var options = archive.DefaultExtractOptions(archiveLimit, dataLimit, entryLimit)
        options.Links = true
        if args[1] == "ziplinks" { return archive.ExtractZIPWith(source, args[3], options) }
        return archive.ExtractTARWith(source, args[3], options)
    }
    if args[1] == "zip" { return archive.ExtractZIP(source, args[3], archiveLimit, dataLimit, entryLimit) }
    if args[1] == "gzip" {
        let reader = gzip.NewReader(source, archiveLimit, archiveLimit)?
        return archive.ExtractTAR(reader, args[3], archiveLimit, dataLimit, entryLimit)
    }
    return archive.ExtractTAR(source, args[3], archiveLimit, dataLimit, entryLimit)
}
func main() { let err = run(); if err != nil { panic(errors.Message(err)) } }
'''


def main():
    root = Path(__file__).resolve().parents[2]
    compiler = root / 'target' / 'debug' / ('zore.exe' if os.name == 'nt' else 'zore')
    checked = 0
    with tempfile.TemporaryDirectory(prefix='zore-extract-') as temporary:
        directory = Path(temporary).resolve()
        source = directory / 'main.ore'
        source.write_text(PROGRAM, encoding='utf-8')
        built = subprocess.run([str(compiler), 'build', str(source)], cwd=directory,
                               capture_output=True, timeout=180)
        assert built.returncode == 0, built.stderr
        executable = directory / ('main.exe' if os.name == 'nt' else 'main')
        input_path, destination = directory / 'input', directory / 'result'

        def run(mode, data, valid=True, expected=None, archive_limit=16777216,
                data_limit=1048576, entry_limit=100, target=None, keep=False):
            nonlocal checked
            target = target or destination
            if not keep and destination.exists():
                shutil.rmtree(destination)
            input_path.write_bytes(data)
            result = subprocess.run([str(executable), mode, str(input_path), str(target),
                                     str(archive_limit), str(data_limit), str(entry_limit)],
                                    capture_output=True, timeout=30,
                                    env={**os.environ, 'ZORE_CHECK_LEAKS': '1'})
            checked += 1
            assert (result.returncode == 0) == valid, (mode, len(data), result.stderr)
            assert b'leak:' not in result.stderr, result.stderr
            assert not list(directory.glob('.zore-stage-*')), list(directory.iterdir())
            if valid:
                actual = {str(p.relative_to(target)).replace(os.sep, '/'): p.read_bytes()
                          for p in target.rglob('*') if p.is_file()}
                assert actual == (expected or {}), (actual, expected)
            elif not keep:
                assert not destination.exists(), list(directory.iterdir())
            return result

        entries = [('dir/', b''), ('dir/é.txt', b'hello\0world'), ('empty', b'')]
        variants = [('tar', tar_bytes(entries)),
                    ('zip', zip_bytes(entries)),
                    ('zip', zip_bytes(entries, streamed=True, compression=zipfile.ZIP_DEFLATED)),
                    ('gzip', gzip.compress(tar_bytes(entries), mtime=0))]
        for mode, encoded in variants:
            run(mode, encoded, expected={'dir/é.txt': b'hello\0world', 'empty': b''}, entry_limit=3)
            run(mode, encoded, valid=False, entry_limit=2)
            run(mode, encoded, valid=False, data_limit=10)
            run(mode, encoded, valid=False, archive_limit=len(encoded) - 1)
            run(mode, encoded[:2047] if mode == 'tar' else encoded[:-1], valid=False)
            corrupt = bytearray(encoded)
            if mode == 'tar':
                corrupt[512 + 512 + 11] = 1
            elif mode == 'gzip':
                corrupt[-8] ^= 1
            else:
                central = corrupt.index(b'PK\x01\x02')
                central = corrupt.index(b'PK\x01\x02', central + 1)
                corrupt[central + 16] ^= 1
            run(mode, corrupt, valid=False)
            destination.mkdir()
            (destination / 'sentinel').write_bytes(b'untouched')
            run(mode, encoded, valid=False, keep=True)
            assert (destination / 'sentinel').read_bytes() == b'untouched'
            shutil.rmtree(destination)

        bad_entries = [
            [('a', b'x'), ('a', b'y')],
            [('a/', b''), ('a/', b'')],
            [('A/x', b'x'), ('a/y', b'y')],
            [('é/x', b'x'), ('É/y', b'y')],
            [('a', b'x'), ('a/b', b'y')],
            [('a/b', b'y'), ('a', b'x')],
            [('safe', b'ok'), ('../escape', b'bad')],
            [('safe', b'ok'), ('CON.txt', b'bad')],
            [('safe', b'ok'), ('a\\b', b'bad')],
            [('safe', b'ok'), ('a//b', b'bad')],
        ]
        with warnings.catch_warnings():
            warnings.simplefilter('ignore', UserWarning)
            for entries in bad_entries:
                run('tar', tar_bytes(entries), valid=False)
                run('zip', zip_bytes(entries, compression=zipfile.ZIP_DEFLATED), valid=False)
        for mode, build in [('tar', tar_bytes), ('zip', zip_bytes)]:
            encoded = build([('a/b', b'x'), ('a/', b'')])
            run(mode, encoded, expected={'a/b': b'x'}, entry_limit=2)
            run(mode, encoded, valid=False, entry_limit=1)
            run(mode, build([]), expected={}, entry_limit=0, data_limit=0)
            deep = '/'.join(['a'] * 64)
            run(mode, build([(deep, b'x')]), expected={deep: b'x'}, entry_limit=64)
            run(mode, build([(deep, b'x')]), valid=False, entry_limit=63)
            run(mode, build([(deep + '/a', b'x')]), valid=False)
            for target in [str(directory) + '/.', str(directory) + '/..']:
                result = run(mode, encoded, valid=False, target=target)
                assert b'invalid' in result.stderr, result.stderr
        outside = directory / 'outside'
        outside.mkdir()
        link = directory / 'link'
        link.symlink_to(outside, target_is_directory=True)
        run('tar', tar_bytes([('escape', b'bad')]), valid=False, target=link / 'result')
        assert not list(outside.iterdir())
        link.unlink()
        destination.symlink_to(outside / 'missing', target_is_directory=True)
        run('zip', zip_bytes([]), valid=False, keep=True)
        assert destination.is_symlink()
        destination.unlink()
        output = io.BytesIO()
        with tarfile.open(fileobj=output, mode='w', format=tarfile.USTAR_FORMAT) as archive:
            info = tarfile.TarInfo('link')
            info.type, info.linkname = tarfile.SYMTYPE, '../outside'
            archive.addfile(info)
        refused = run('tar', output.getvalue(), valid=False)
        assert b'archive: links are not extracted' in refused.stderr, refused.stderr
        output = io.BytesIO()
        with tarfile.open(fileobj=output, mode='w', format=tarfile.USTAR_FORMAT) as archive:
            info = tarfile.TarInfo('hard')
            info.type, info.linkname = tarfile.LNKTYPE, 'tool'
            archive.addfile(info)
        run('tar', output.getvalue(), valid=False)
        output = io.BytesIO()
        with tarfile.open(fileobj=output, mode='w', format=tarfile.USTAR_FORMAT) as archive:
            for name, mode, data in [('bin/tool', 0o755, b'#!x'), ('bin/only-group', 0o610, b'g'), ('notes', 0o644, b'n')]:
                info = tarfile.TarInfo(name)
                info.mode, info.size = mode, len(data)
                archive.addfile(info, io.BytesIO(data))
        run('tar', output.getvalue(), expected={'bin/tool': b'#!x', 'bin/only-group': b'g', 'notes': b'n'})
        if os.name != 'nt':
            assert destination.joinpath('bin/tool').stat().st_mode & 0o777 == 0o700
            assert destination.joinpath('bin/only-group').stat().st_mode & 0o777 == 0o700
            assert destination.joinpath('notes').stat().st_mode & 0o777 == 0o600
        output = io.BytesIO()
        with zipfile.ZipFile(output, 'w') as archive:
            for name, system, mode in [('unix-tool', 3, 0o100755), ('dos-file', 0, 0o100755), ('unix-file', 3, 0o100644)]:
                info = zipfile.ZipInfo(name)
                info.create_system = system
                info.external_attr = mode << 16
                archive.writestr(info, name.encode())
        run('zip', output.getvalue(), expected={'unix-tool': b'unix-tool', 'dos-file': b'dos-file', 'unix-file': b'unix-file'})
        if os.name != 'nt':
            modes = {name: destination.joinpath(name).stat().st_mode & 0o777 for name in ['unix-tool', 'dos-file', 'unix-file']}
            assert modes == {'unix-tool': 0o700, 'dos-file': 0o600, 'unix-file': 0o600}, modes
        output = io.BytesIO()
        with zipfile.ZipFile(output, 'w') as archive:
            info = zipfile.ZipInfo('link')
            info.create_system = 3
            info.external_attr = 0o120777 << 16
            archive.writestr(info, '../outside')
        run('zip', output.getvalue(), valid=False)
        def linked(entries):
            output = io.BytesIO()
            with tarfile.open(fileobj=output, mode='w', format=tarfile.PAX_FORMAT) as archive:
                for name, kind, value in entries:
                    info = tarfile.TarInfo(name)
                    if kind == 'f':
                        info.size = len(value)
                        archive.addfile(info, io.BytesIO(value))
                        continue
                    info.type = {'d': tarfile.DIRTYPE, 's': tarfile.SYMTYPE, 'h': tarfile.LNKTYPE}[kind]
                    info.linkname = value if kind != 'd' else ''
                    archive.addfile(info)
            return output.getvalue()

        hard_only = linked([('a/file', 'f', b'hi'), ('a/hard', 'h', 'a/file'), ('dot', 'h', './a/file')])
        run('tarlinks', hard_only, expected={'a/file': b'hi', 'a/hard': b'hi', 'dot': b'hi'})
        assert destination.joinpath('a/hard').stat().st_ino == destination.joinpath('a/file').stat().st_ino or os.name == 'nt'
        refused = run('tar', hard_only, valid=False)
        assert b'archive: links are not extracted' in refused.stderr
        symlinks = linked([('a/file', 'f', b'hi'), ('a/rel', 's', 'file'), ('top', 's', 'a/file'),
                           ('up', 's', 'a/../a/file'), ('dirlink', 's', 'a')])
        if os.name == 'nt':
            run('tarlinks', symlinks, valid=False)
        else:
            run('tarlinks', symlinks, expected={'a/file': b'hi', 'a/rel': b'hi', 'top': b'hi', 'up': b'hi'})
            links = {name: os.readlink(destination / name) for name in ['a/rel', 'top', 'up', 'dirlink']}
            assert links == {'a/rel': 'file', 'top': 'a/file', 'up': 'a/../a/file', 'dirlink': 'a'}, links
        output = io.BytesIO()
        with zipfile.ZipFile(output, 'w') as archive:
            archive.writestr('file', b'z')
            info = zipfile.ZipInfo('link')
            info.create_system, info.external_attr = 3, 0o120777 << 16
            archive.writestr(info, 'file')
        if os.name == 'nt':
            run('ziplinks', output.getvalue(), valid=False)
        else:
            run('ziplinks', output.getvalue(), expected={'file': b'z', 'link': b'z'})
            assert os.readlink(destination / 'link') == 'file'
        for entries in [[('a/x', 's', '../../outside')], [('x', 's', '/etc/passwd')], [('x', 's', 'a\\b')],
                        [('x', 's', 'C:/x')], [('a', 's', '.'), ('b', 's', 'a/..')],
                        [('b', 's', 'a/../x'), ('a', 's', '.')], [('h', 'h', 'missing')],
                        [('d/', 'd', ''), ('h', 'h', 'd')], [('h', 'h', 'f'), ('f', 'f', b'x')],
                        [('d', 's', '.'), ('d/x', 'f', b'x')], [('h', 'h', '../x')]]:
            run('tarlinks', linked(entries), valid=False)
        assert not list(outside.iterdir())
        def tree():
            return {str(p.relative_to(destination)).replace(os.sep, '/'): p.read_bytes()
                    for p in destination.rglob('*') if p.is_file()}

        if destination.exists() or destination.is_symlink():
            shutil.rmtree(destination) if destination.is_dir() and not destination.is_symlink() else destination.unlink()
        destination.mkdir()
        (destination / 'keep').mkdir()
        (destination / 'keep' / 'old.txt').write_bytes(b'old')
        (destination / 'top.txt').write_bytes(b'top')
        incoming = [('keep/', b''), ('keep/new.txt', b'new'), ('fresh/deep/file', b'deep')]
        for mode, data in [('tarmerge', tar_bytes(incoming)), ('zipmerge', zip_bytes(incoming))]:
            run(mode, data, keep=True, expected={'keep/old.txt': b'old', 'top.txt': b'top', 'keep/new.txt': b'new', 'fresh/deep/file': b'deep'})
            before = tree()
            run(mode, data, valid=False, keep=True)
            assert tree() == before
            shutil.rmtree(destination / 'fresh')
            (destination / 'keep' / 'new.txt').unlink()
        conflict = tar_bytes([('added.txt', b'a'), ('keep/old.txt', b'replaced')])
        result = run('tarmerge', conflict, valid=False, keep=True)
        assert b'already contains keep/old.txt' in result.stderr, result.stderr
        assert tree() == {'keep/old.txt': b'old', 'top.txt': b'top'}
        run('tarmerge', tar_bytes([('top.txt/', b'')]), valid=False, keep=True)
        run('tarmerge', tar_bytes([('keep', b'file over folder')]), valid=False, keep=True)
        assert tree() == {'keep/old.txt': b'old', 'top.txt': b'top'}
        if os.name != 'nt':
            (destination / 'linked').symlink_to(outside, target_is_directory=True)
            run('tarmerge', tar_bytes([('linked/escape', b'x')]), valid=False, keep=True)
            assert not list(outside.iterdir())
            (destination / 'linked').unlink()
        shutil.rmtree(destination)
        destination.write_bytes(b'file')
        run('tarmerge', tar_bytes([('a', b'x')]), valid=False, keep=True)
        destination.unlink()
        if os.name != 'nt':
            destination.symlink_to(outside, target_is_directory=True)
            run('tarmerge', tar_bytes([('a', b'x')]), valid=False, keep=True)
            assert not list(outside.iterdir())
            destination.unlink()
        run('tarmerge', tar_bytes([('a', b'x')]), expected={'a': b'x'})
        payload = b'a' * 100000
        compressor = zlib.compressobj(wbits=-15)
        raw = compressor.compress(payload) + compressor.flush()
        run('zip', raw_zip(payload, raw, declared=1), valid=False)
        if sys.platform == 'darwin':
            run('zip', zip_bytes([('é/x', b'a'), ('e\u0301/y', b'b')]), valid=False)
        print(f'extraction interoperability and rollback: {checked} cases passed')


if __name__ == '__main__':
    main()
