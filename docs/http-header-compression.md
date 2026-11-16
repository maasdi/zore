# HTTP header compression (HPACK)

HTTP/2 does not send headers as text. It packs them with a scheme called HPACK:
common headers become a one-byte number, repeated headers are remembered in a
small table, and other text can be squeezed with a fixed code. `std/net/http/hpack`
packs and unpacks such header blocks, with limits so that a tiny block cannot
turn into a huge list. The full contract is in the
[specification](../spec/language-spec.md#3748-http-header-compression--locked).

This package is a building block for HTTP/2. **Zore does not speak HTTP/2 yet**;
see [decision 0004](decisions/0004-http2-client.md).

## Packing and unpacking

```ore
package main

import "std/net/http/hpack"

func show() error {
    var encoder = hpack.NewEncoder()
    var decoder = hpack.NewDecoder(hpack.DefaultTableBytes, 16384)?
    let fields = Array<hpack.Field>{
        hpack.Field{Name: ":method", Value: "GET"},
        hpack.Field{Name: ":path", Value: "/status"},
        hpack.Field{Name: "accept", Value: "text/plain"},
    }
    let block = encoder.Encode(fields[:])
    let back = decoder.Decode(block[:])?
    for field in back {
        println(field.Name + ": " + field.Value)
    }
    return nil
}

func main() {
    let err = show()
    if err != nil {
        panic("header block failed")
    }
}
```

## Unpacking rules

- A decoder belongs to one connection and must see every header block of that
  connection, in order, because blocks refer to headers remembered from earlier
  ones.
- `NewDecoder(tableBytes, listBytes)` sets two limits. `tableBytes` (at most
  65,536) is how much the decoder is willing to remember. `listBytes` is the
  largest unpacked list, counting each header as its name, its value and 32
  bytes.
- Any failure is final: the decoder then answers `hpack: decoder failed`,
  because its memory may no longer match the sender's. HTTP/2 treats this as the
  end of the connection.

| Error | Meaning |
| --- | --- |
| `hpack: truncated header block` | The block ends in the middle of a number or a text. |
| `hpack: integer too large` | A number uses more than five bytes. |
| `hpack: invalid table index` | The block refers to a header that is not in the tables. |
| `hpack: table size update too large` | The sender asked for more memory than `tableBytes`. |
| `hpack: misplaced table size update` | A size change after the first header, or more than two. |
| `hpack: invalid Huffman data` | Squeezed text is broken or has wrong padding. |
| `hpack: invalid field text` | A name or value is not valid UTF-8. |
| `hpack: header list too large` | The unpacked list is larger than `listBytes`. |

## Packing rules

- The encoder uses the fixed table of common headers and squeezes text when
  that is shorter. It does not make the receiver remember anything, so it needs
  no memory per connection.
- `authorization`, `proxy-authorization`, `cookie` and `set-cookie` are marked
  "never remember", which tells proxies along the way not to store them.
- When the other side lowers its table limit below 4,096 bytes, call
  `SetTableLimit`; the next block then starts with the notice the protocol
  requires.
- The package does not check header names or values. Lowercase names and other
  HTTP rules are the job of the protocol code that uses it.
