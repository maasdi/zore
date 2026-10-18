# Greeting library metadata

This local sample illustrates optional release metadata. It is not a published
`greeting/v1.2.3` release. The project uses the repository's Apache-2.0 license.

From the repository root:

```sh
zore check --release --tag greeting/v1.2.3 examples/release-metadata
zore check examples/release-metadata/greeting.ore
zore test examples/release-metadata
```

The first command checks metadata and the supplied tag string. The other commands
check and test source independently. The helper never creates, looks up or pushes
Git tags. See [release metadata](../../docs/release-metadata.md) for the contract.
