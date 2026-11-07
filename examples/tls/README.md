# TLS examples

`server/` and `client/` are small protocol examples for the maintained `tls`
package in `packages/tls`, consumed here by local path. The package readme
describes the API, trust roots and limits.

```sh
cd examples/tls/server && zore install --offline && zore run --locked main.ore
cd examples/tls/client && zore install --offline && zore build --locked main.ore
./main ../../../packages/tls/testdata/roots.pem
```

`server/` accepts one loopback connection on port 8443 and answers with a fixed
HTTP response. `client/` sends one bounded HTTP/1.1 request over verified TLS
and prints the first response read. Without a roots file the client trusts the
system roots. The fixture certificates and keys are public test data.
