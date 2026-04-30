# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

# AZPFS — Networked FUSE Filesystem

CSCI 1680 Final Project, Brown University, Spring '26.

A networked filesystem over TCP using Linux's FUSE interface. The client is a FUSE daemon that
forwards kernel filesystem calls over the wire to a server, which serves them against a real
on-disk filesystem.

## Crate layout

Single crate (`azpfs`) with one library and two binaries:

```
src/lib/        # libazpfs — all protocol, server, and filesystem logic + tests
src/client/     # binary azpfsd — mounts FUSE, delegates to libazpfs ClientHandler
src/server/     # binary azpfs-server — listens on TCP, delegates to libazpfs server logic
```

## Build & run

```bash
cargo build
cargo test

# Run server (serves <root_dir> over TCP on <port>)
cargo run --bin azpfs-server -- <port> <root_dir>

# Mount client (mounts at <mountpoint>, connecting to server at <host>:<port>)
cargo run --bin azpfsd -- <host>:<port> <mountpoint>

# Unmount
fusermount -u <mountpoint>
```

## Testing

Tests live in `src/lib/`. Two layers:


A `FsBackend` trait abstracts the server-side filesystem, with `RealFs` (wrapping `tokio::fs`)
as the primary implementation. This allows server logic to be tested without touching disk when
a `MemFs` backend mock is substituted. Tests also use an in-memory `Transport` implementation as
an in-process socket shim — no real TCP, no FUSE mount needed.

```bash
cargo test                    # run all tests
cargo test <test_name>        # run a single test by name
cargo test -- --nocapture     # show println! output
```

## Key architecture

### Protocol
- Request-response over a single TCP stream
- Messages are **asynchronous**: the server may respond out of order to prevent large reads
  from blocking short operations
- Large payloads (e.g., file reads) may be chunked across multiple messages
- Each filesystem operation maps roughly 1:1 to a request type (although other reqs/responses exist for session setup, error handling, etc)

### Server
- `Arc<ServerState>` shared across per-connection `handle_client` tasks (spawned by Tokio)

### Client (`ClientHandler<T, F>`)
- Parameterized over transport layer `T: Transport` and filesystem backend `F: FsBackend` (see below)
- Implements the `fuser::Filesystem` trait, forwarding calls to the server via `T`

### `FsBackend` trait
Async trait with methods mirroring the FUSE operations the server needs to serve:
`getattr`, `readdir`, `read`, `write`, and friends.

Implementations:
- `RealFs` — wraps `tokio::fs`, rooted at a `PathBuf`
- `MemFs` *(stretch)* — `HashMap<PathBuf, Vec<u8>>` behind `tokio::sync::RwLock`

### `Transport` trait
Abstraction over the network transport layer. Two main implementations:
- **TCP**. Actual TCP, used "normally."
- **In-memory shim** via `tokio` channels. Used for integration testing without TCP overhead (even on loopback).

## FUSE operations in scope

Core (required):
- `lookup`, `getattr`, `setattr`
- `readdir`, `mkdir`, `rmdir`
- `create`, `open`, `read`, `write`, `flush`, `release`
- `unlink`, `rename`

Stretch:
- `link`, `symlink`, `readlink` (inode links)
- `getlk`, `setlk` (POSIX flock)

## Project scope & deadlines

- **Proposal**: April 27, 2026 *(submitted)*
- **Final submission**: May 8, 2026
- Working solo

The implementation does not need to be production-grade. The goal is a working end-to-end
demonstration: mount the client, perform file CRUD operations, have them reflected on the
server's filesystem.
