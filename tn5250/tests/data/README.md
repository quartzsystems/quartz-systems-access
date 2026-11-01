# Test certificates

Throwaway certificates for the TLS unit tests in `tn5250/src/telnet/tls.rs`.
The private keys here protect nothing and must never be used anywhere else.

- `test-ca.pem`: a test CA (its key was discarded after signing).
- `server.pem`, `server.key`: `CN=localhost`, signed by the test CA, for
  `localhost` and `127.0.0.1`.
- `selfsigned.pem`, `selfsigned.key`: a self-signed `CN=selfsigned`, for the
  "Trust for this system" path.

Made with OpenSSL: P-256 keys, 100-year validity.
