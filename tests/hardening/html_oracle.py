"""Compare HTML escaping with Python and parse fixed text/attribute contexts."""

import html
from html.parser import HTMLParser
import os
from pathlib import Path
import random
import struct
import subprocess
import tempfile


PROGRAM = '''package main
import "std/encoding/binary"
import "std/errors"
import "std/html"
import "std/os"
import "std/strings"

func run() error {
    let args = os.Args()
    let input = os.ReadFile(args[1])?
    var output = Array<byte>{}
    var offset = 0
    for offset < input.len() {
        if input.len() - offset < 4 { return error("short frame") }
        let size = int(binary.LittleEndian.Uint32(input[offset:offset + 4])?)
        offset += 4
        if size > input.len() - offset { return error("short text") }
        let text = strings.FromBytes(input[offset:offset + size])?
        offset += size
        let escaped = html.EscapeString(text)
        binary.LittleEndian.AppendUint32(output, uint32(escaped.len()))
        for var index = 0; index < escaped.len(); index += 1 {
            output.push(escaped[index])
        }
    }
    return os.WriteFile(args[2], output[:], 0o600)
}
func main() {
    let err = run()
    if err != nil { panic(errors.Message(err)) }
}
'''


class ContextParser(HTMLParser):
    def __init__(self):
        super().__init__(convert_charrefs=True)
        self.events = []
        self.text = []

    def handle_starttag(self, tag, attrs):
        self.events.append(('start', tag, attrs))

    def handle_endtag(self, tag):
        self.events.append(('end', tag))

    def handle_data(self, data):
        self.text.append(data)

    def handle_comment(self, data):
        self.events.append(('comment', data))

    def handle_decl(self, decl):
        self.events.append(('declaration', decl))


def frames(payloads):
    return b''.join(struct.pack('<I', len(value)) + value for value in payloads)


def unframe(data):
    values = []
    offset = 0
    while offset < len(data):
        assert len(data) - offset >= 4, 'short output frame'
        size, = struct.unpack_from('<I', data, offset)
        offset += 4
        assert size <= len(data) - offset, 'short output text'
        values.append(data[offset:offset + size].decode('utf-8'))
        offset += size
    return values


def main():
    rng = random.Random(226)
    cases = ['', '&<>"\'', '&amp;&lt;&#39;&#x27;&quot;', '漢字🙂e\u0301',
             '\0\r\n\t\u2028', '</p><script>alert(1)</script>',
             '" autofocus onfocus="alert(1)', "' onmouseover='alert(1)",
             '<!-- --> <!DOCTYPE html>', 'javascript:alert(1) ` = / \\',
             '"' * 50000, '🙂éa' * 50000]
    cases.extend(chr(value) for value in range(256))
    alphabet = list('&<>"\' abc\t\r\n\0;#x0123456789')
    alphabet.extend(['漢', '字', '🙂', 'é', '\u0301', '\u2028', '\U0010ffff'])
    for _ in range(1000):
        cases.append(''.join(rng.choice(alphabet) for _ in range(rng.randrange(256))))
    root = Path(__file__).resolve().parents[2]
    compiler = root / 'target' / 'debug' / ('zore.exe' if os.name == 'nt' else 'zore')
    with tempfile.TemporaryDirectory(prefix='zore-html-') as temporary:
        directory = Path(temporary)
        source = directory / 'main.ore'
        source.write_text(PROGRAM, encoding='utf-8')
        built = subprocess.run([str(compiler), 'build', str(source)], cwd=directory,
                               capture_output=True, timeout=180)
        assert built.returncode == 0, built.stderr
        executable = directory / ('main.exe' if os.name == 'nt' else 'main')
        input_path, output_path = directory / 'input', directory / 'output'
        input_path.write_bytes(frames(text.encode('utf-8') for text in cases))
        command = [str(executable), str(input_path), str(output_path)]
        environment = {**os.environ, 'ZORE_CHECK_LEAKS': '1'}
        result = subprocess.run(command, capture_output=True, timeout=30, env=environment)
        assert result.returncode == 0 and not result.stderr, result.stderr
        escaped = unframe(output_path.read_bytes())
        assert len(escaped) == len(cases)
        for text, actual in zip(cases, escaped):
            expected = html.escape(text, quote=True).replace('&#x27;', '&#39;')
            assert actual == expected, (text, expected, actual)
            assert len(actual.encode('utf-8')) <= 6 * len(text.encode('utf-8'))
            for quote in ['"', "'"]:
                parser = ContextParser()
                parser.feed(f'<p title={quote}{actual}{quote}>{actual}</p>')
                parser.close()
                assert parser.events == [('start', 'p', [('title', text)]), ('end', 'p')], (text, parser.events)
                assert ''.join(parser.text) == text, (text, parser.text)
        for invalid in [b'\xff', b'\xc0\xaf', b'\xed\xa0\x80', b'\xf4\x90\x80\x80']:
            input_path.write_bytes(frames([invalid]))
            result = subprocess.run(command, capture_output=True, timeout=30, env=environment)
            assert result.returncode != 0, invalid
            assert b'leak:' not in result.stderr, result.stderr
    print(f'HTML oracle: {len(cases)} UTF-8 cases, both quoted contexts, 4 invalid-byte refusals')


if __name__ == '__main__':
    main()
