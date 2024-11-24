# bullseye
Bullseye is an upload target designed for use with Chromebot v2. However, it is very generic, and can be used in almost any kind of circumstance.

## Warning
Bullseye is still experimental. It does not currently have full testing, and is not yet used in any production software. Use with caution.

## General idea
Bullseye was based heavily on this design doc: https://s3.services.ams.aperture-laboratories.science/rewby/public/85ba4e62-8d5a-4b81-b410-08575da464b6/HTTP%20Packer%20Design%20Doc.pdf. Reading through that design doc will help you understand how Bullseye works. Please note that the protocol is not exactly the same as is outlined in the design doc.

This repository specifically includes the frontend (the `server` directory) and the client (the `client` directory), which are the generic parts. There is currently no director. Other parts are pipeline-specific; when writing them, you will probably want to link to the `common` crate provided in this repo.

## Testing
- **Server**: Run `cargo test` in the server directory. The error handling tests are designed to work on a 50MiB filesystem. Mount a 50MiB tmpfs to the server data directory before starting.

## Known issues
The code isn't great, because I used this project as a chance to become better with Rust. It might be a little hard to read at times. Patches welcome! :-)

## License

Licensed under either of

 * Apache License, Version 2.0, ([LICENSE-APACHE](LICENSE-APACHE) or https://www.apache.org/licenses/LICENSE-2.0)
 * MIT license ([LICENSE-MIT](LICENSE-MIT) or https://opensource.org/licenses/MIT)

at your option.

### Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any
additional terms or conditions.
