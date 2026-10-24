# Cryptographic utilities example

The `crypto/` project is the external maintained `crypto-v1` package. The `app/`
project consumes it by local path and prints a verified HMAC and digest length.
See [the library guide](../../docs/crypto.md) for setup, provider provenance,
ownership, errors, supported hosts and scope. Portable application and library
locks are checked in; build/run/test require native tools,
while frontend `check` works without them. All checked-in example inputs are
public test data, never production secrets.
