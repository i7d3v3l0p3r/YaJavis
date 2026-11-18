# Sources and licenses

The module uses rustls from crates.io for ordinary TLS and a separate connection-scoped
REALITY fork for REALITY/Vision. No workspace-wide TLS patch is installed.

- https://github.com/cfal/rustls at `f1a23c187330386e9e7f14a21dbae368034dc430`
  (`Apache-2.0 OR ISC OR MIT`), vendored in `jarvis/vendor/reality-tls` with its licenses.
  Jarvis adds a connection-scoped ClientHello shaping hook before authentication/transcript hashing.
- Chrome 133 and Firefox 120 profile reference: https://github.com/refraction-networking/utls
  (`HelloChrome_133` and `HelloFirefox_120` in `u_parrots.go`); BSD notice retained as `UTLS-LICENSE.txt`.
- Pure Rust ML-KEM-768: RustCrypto `ml-kem` (`Apache-2.0 OR MIT`), pinned in Cargo.lock.
  Hybrid shares follow https://datatracker.ietf.org/doc/draft-ietf-tls-ecdhe-mlkem/.
- Protocol reference: https://github.com/XTLS/REALITY and the VLESS Addons schema in
  https://github.com/XTLS/Xray-core/blob/main/proxy/vless/encoding/addons.proto.
- REALITY authentication and directional Vision handoff were implemented with reference
  to https://github.com/cfal/shoes at `7680938b96a245a21d9e481ad009205b956e4436`.
  The adapted authentication logic retains the following MIT notice.

Copyright (c) 2021-2023 Alex Lau <github@alau.ca>

Permission is hereby granted, free of charge, to any person obtaining
a copy of this software and associated documentation files (the
"Software"), to deal in the Software without restriction, including
without limitation the rights to use, copy, modify, merge, publish,
distribute, sublicense, and/or sell copies of the Software, and to
permit persons to whom the Software is furnished to do so, subject to
the following conditions:

The above copyright notice and this permission notice shall be
included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND,
EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF
MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND
NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE
LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION
OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION
WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
