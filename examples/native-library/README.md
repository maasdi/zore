# Native checksum library

`adler32` is an independently consumable Zore library project. Its manifest
registers the compiler's reviewed `adler32-v1` recipe. The library owns its C
implementation, header, Rust bridge and binding contract in `adler32/native/`;
its public wrapper owns and borrows resources using ordinary Zore code. `app`
depends on that project and prints the non-cryptographic Adler-32 checksum of
Wikipedia: `300286872`.

From the repository root:

```sh
zore install --offline examples/native-library/app
zore check --locked --offline examples/native-library/app/main.ore
zore run --locked --offline examples/native-library/app/main.ore
zore install --offline examples/native-library/adler32
zore test --locked --offline examples/native-library/adler32
```

The committed locks select the reviewed native recipe. Regenerate them with
`zore lock --offline <project>` after a reviewed registry recipe change. Modified
native files require registry review; a new lock alone cannot approve them. Native
builds need clang with C11 headers and the usual Rust host toolchain; checking
needs neither. See [native libraries](../../docs/native-libraries.md) for the
supported hosts, ownership/error rules and current curated-adapter limits.

`native/registration.rs` is a CI lint harness for the Rust bridge. Production
builds generate their own registration from validated package sources.
