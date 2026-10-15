# Native checksum library

`adler32` is an independently consumable Zore library project. Its manifest
registers the compiler's reviewed `adler32-v1` C/Rust adapter; its public wrapper
owns and borrows resources using ordinary Zore code. `app` depends on that project
and prints the non-cryptographic Adler-32 checksum of Wikipedia: `300286872`.

From the repository root:

```sh
zore install --offline examples/native-library/app
zore check --locked --offline examples/native-library/app/main.ore
zore run --locked --offline examples/native-library/app/main.ore
zore install --offline examples/native-library/adler32
zore test --locked --offline examples/native-library/adler32
```

The committed locks select the reviewed native recipe. Regenerate them with
`zore lock --offline <project>` after an intentional recipe change. Native
builds need clang with C11 headers and the usual Rust host toolchain; checking
needs neither. See [native libraries](../../docs/native-libraries.md) for the
supported hosts, ownership/error rules and current curated-adapter limits.
