# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

# AZPFS — Networked FUSE Filesystem

CSCI 1680 Final Project, Brown University, Spring '26.

A networked filesystem over TCP using Linux's FUSE interface. The client is a FUSE daemon that
forwards kernel filesystem calls over the wire to a server, which serves them against a real
on-disk filesystem.

This is meant to be run over Linux, in the course ubuntu container.

## Crate layout

Single crate (`azpfs`) with one library (`libazpfs`) and two binaries:

```
src/core/           # libazpfs (lib root: src/core/mod.rs)
src/core/client/    #   client-side logic (FUSEFilesystem / ClientHandler)
src/core/server/    #   server-side logic (handle_client, FsBackend impls)
src/client/main.rs  # binary azpfsd — mounts FUSE, delegates to libazpfs
src/server/main.rs  # binary azpfs-server — listens on TCP, delegates to libazpfs
```

## Build & run

```bash
cargo build
cargo test

# Run server (serves <root_dir> over TCP on <port>)
cargo run --bin azpfs-server -- <port> <root_dir>

# Mount client (mounts at <mountpoint>, connecting to server at <host>:<port>)
cargo run --bin azpfsd -- <host>:<port> <mountpoint>
```

## Testing

Tests live in `src/core/`. Two layers:


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

#### Session lifecycle
1. Client connects over TCP and sends `INIT_REQ` (version `0x00`)
2. Server replies with `INIT_RES` (accepted = 1); further `INIT_REQ`s get `E_INVALID`
3. Session ends when the TCP socket closes (client may send RST or do normal TCP close)
4. `open`/`release` are no-ops — reads and writes address inodes directly without opening first

#### Request IDs
- Each client request carries a unique 32-bit Request ID
- IDs must be unique among all *pending* (unanswered) requests
- IDs may be reused after exhausting the 32-bit space (wrap-around)

#### Message type table (spec v1.0)

| Type | ID | Direction | Description |
|---|---|---|---|
| `ERROR` | `0x00` | S→C | Error response; carries error code + ASCII message |
| `INIT_REQ` | `0x01` | C→S | Session initiation |
| `INIT_RES` | `0x02` | S→C | Session accepted |
| `LOOKUP_REQ` | `0x03` | C→S | Look up dir entry by name → inode |
| `LOOKUP_RES` | `0x04` | S→C | Returns inode number (also used for `CREATE_REQ`) |
| `GET_ATTR_REQ` | `0x05` | C→S | Get file attributes |
| `FILE_ATTR_RES` | `0x06` | S→C | File attributes (size, times, mode, uid, gid, …) |
| `SET_ATTR_REQ` | `0x07` | C→S | Set file attributes (field mask selects which fields) |
| `SUCCESS_RES` | `0x08` | S→C | Generic success (no data) |
| `STATS_REQ` | `0x09` | C→S | Get filesystem statistics |
| `STATS_RES` | `0x0A` | S→C | Filesystem statistics (blocks, inodes, etc.) |
| `CREATE_REQ` | `0x0B` | C→S | Create file or directory; returns `LOOKUP_RES` |
| `READ_REQ` | `0x0C` | C→S | Read bytes from inode |
| `READ_RES` | `0x0D` | S→C | Chunk of read data (chunked, out-of-order OK) |
| `WRITE_REQ` | `0x0E` | C→S | Write bytes to inode; returns `SUCCESS_RES` |
| `READDIR_REQ` | `0x0F` | C→S | Read directory entries; server replies with `READ_RES` chunks |
| `RM_REQ` | `0x10` | C→S | Remove inode (file or directory, recursive); returns `SUCCESS_RES` |
| `MOVE_REQ` | `0x11` | C→S | Move/rename inode; returns `SUCCESS_RES` |

#### Chunked reads (`READ_RES`)
- A single `READ_REQ` or `READDIR_REQ` may produce multiple `READ_RES` chunks
- Each chunk carries: `Total Length`, `EOF` flag, `Chunk Length`, `Chunk Offset`, `Data`
- Chunks may arrive out of order; client must reassemble by `Chunk Offset`
- For `READDIR_REQ`, the payload is packed directory entries: `(inode u64, file_type u8, name_len u8, name bytes)`

#### `SET_ATTR_REQ` field mask (bits 0–5)
Bit 0 = Size, Bit 1 = Access Time, Bit 2 = Modification Time, Bit 3 = Permissions, Bit 4 = UID, Bit 5 = GID

#### `FILE_ATTR_RES` file type codes
`0x00` Named pipe · `0x01` Char device · `0x02` Block device · `0x03` Directory · `0x04` Regular file · `0x05` Symlink · `0x06` Unix socket

#### Error codes
| Code | Name | Meaning |
|---|---|---|
| `0x00` | `E_INTERNAL` | Miscellaneous server-side error |
| `0x01` | `E_INVALID` | Malformed or unprocessable request |
| `0x02` | `E_NOTFOUND` | Requested inode does not exist |
| `0x03` | `E_EXISTS` | Inode already exists at the destination |

#### `MOVE_REQ` semantics
- Destination exists and is a **directory** → `E_EXISTS`
- Destination exists and is a **file** → overwrite
- Source and destination are different types → `E_INVALID`

#### `RM_REQ` semantics
- On a file: simple unlink
- On a directory: recursive removal (equivalent to `rm -rf`)

### Server
- `Arc<ServerState>` shared across per-connection `handle_client` tasks (spawned by Tokio)

### Client (`ClientHandler<T, F>`)
- Parameterized over transport layer `T: Transport` and filesystem backend `F: FsBackend` (see below)
- Implements the `fuser::Filesystem` trait, forwarding calls to the server via `T`

### `Transport` trait
First-class abstraction over the network layer, on equal footing with `FsBackend`. Two implementations:
- **TCP** — wraps a real `TcpStream`, used in production
- **In-memory shim** — backed by `tokio::io::duplex`; no real TCP, no OS sockets, used in tests

### `FsBackend` trait
First-class abstraction over the server-side filesystem, on equal footing with `Transport`. Async trait with methods mirroring FUSE operations: `getattr`, `readdir`, `read`, `write`, and friends.

Implementations:
- `RealFs` — wraps `tokio::fs`, rooted at a `PathBuf`
- `MemFs` *(stretch)* — `HashMap<PathBuf, Vec<u8>>` behind `tokio::sync::RwLock`

### `ClientHandler<T: Transport, F: FsBackend>`
Generic over **both** `Transport` and `FsBackend`. Implements `fuser::Filesystem`, forwarding FUSE calls to the server via `T`.

This dual genericity is what makes the test harness work: tests construct a `ClientHandler` with the in-memory shim transport and a `RealFs` (or `MemFs`) backend, wire the other end to a `handle_client` task, and exercise the full client-server path in-process — no FUSE mount, no TCP. **Always understand both traits before writing any code involving `ClientHandler` or tests.**

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
