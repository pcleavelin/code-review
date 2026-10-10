---
name: add-data-source
description: Add a new way for the review tool to read or write data outside the process - a file format, a program it runs, a forge API, a network or IPC protocol.
---

# Add a data source

Each source of outside data has one `io-*` crate with one entry point per direction.

1. Check whether an existing io crate owns the data already (`api/io-*.api`). If it does,
   extend it there instead of adding a crate. A new forge, or a new forge endpoint, goes in
   `io-forge`; anything git does goes in `io-vcs`; the review files are `io-review`'s.
2. A new crate: add it to the workspace and its edges to the crate graph
   (`xtask/src/arch.rs` DEPENDENCIES): the io crate may depend on `domain` and the io crates
   it writes or runs through; the crates that call it get an edge to it (usually `session`).
3. Build the crate:
   - `src/wire.rs`: types that mirror the external format exactly, and the parser/printer;
   - `src/convert.rs`: wire to domain and back; nothing else names a wire type;
   - `src/lib.rs`: the entry points (`Store::load`, `Store::save`, `Client::list`, ...) and one
     error enum carrying data, with a `Display` that says what to do about it.
   A process start goes through `io-process`, a file write through `io-store`. An environment
   variable is read in the one file lint L7 allows for the crate. A secret (a token) never goes
   on a command line or into a `Debug`/`Display`: it is passed on a process's stdin.
4. Tests: a round trip over real samples of the format, byte for byte, and no test touches the
   network.
5. `cargo xtask api`, then `finish`.
