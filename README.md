# AZPFS — Networked FUSE Filesystem

A networked filesystem over TCP, written in Rust on Linux's FUSE interface.

The client is a FUSE daemon that forwards kernel filesystem calls over the wire to a server.
The server runs those calls against a real on-disk filesystem and returns the result.

The protocol is a custom binary wire format. It is fully asynchronous: the server may answer
out of order, so a large read never blocks a short operation behind it. Large payloads are
chunked, and the client reassembles them by offset. See [Protocol.md](Protocol.md) for the
full wire format, and the [message type table](#message-type-table-spec-v10) below for a summary.

Built for CSCI 1680 (Computer Networks) at Brown University. Runs on Linux; needs FUSE.

## Crate layout

Single crate (`azpfs`) with one library (`libazpfs`) and two binaries:

```
src/core/           # libazpfs (lib root: src/core/mod.rs)
src/core/client/    #   client-side logic (FUSEFilesystem / ClientHandler)
src/core/server/    #   server-side logic (handle_client, handle_msg)
src/client/main.rs  # binary azpfsd — mounts FUSE, delegates to libazpfs
src/server/main.rs  # binary azpfs-server — listens on TCP, delegates to libazpfs
```

## Build & run

```bash
cargo build
cargo test

# Run server (serves <root_dir> over TCP on <addr:port>)
cargo run --bin azpfs-server -- <addr:port> <root_dir>

# Mount client (mounts at <mountpoint>, connecting to server at <addr:port>)
cargo run --bin azpfsd -- <mountpoint> <addr:port>
```

## Testing

Tests live in `src/core/tests.rs` (module declared in `src/core/mod.rs`).

### Test harness pattern

All client-server tests use `tokio::io::duplex` to link a `ClientHandler` to a `handle_client`
task in-process — no real TCP, no FUSE mount needed. Use the `setup()` helper:

```rust
async fn setup() -> (ClientHandler<BoxWriter>, TempDir) {
    let dir = TempDir::new().expect("failed to create temp dir");
    let fs = Arc::new(Mutex::new(DiskFs::new(dir.path().to_path_buf())));
    let (client_reader, server_writer) = tokio::io::duplex(4096);
    let (server_reader, client_writer) = tokio::io::duplex(4096);
    tokio::spawn(handle_client(server_reader, server_writer, fs));
    let handler = timeout(Duration::from_secs(5), ClientHandler::new(client_reader, client_writer))
        .await
        .expect("setup timed out")
        .expect("ClientHandler::new failed");
    (handler, dir)
}
```

`handle_client` takes a third argument: an `Arc<Mutex<impl FsBackend>>`. The `TempDir` must be
kept alive for the duration of the test — dropping it deletes the backing directory. The two
duplex pairs are **crossed**: client reads from one end, server writes to the other, and vice
versa. `ClientHandler::new` performs the INIT handshake internally, so `setup()` returning `Ok`
already validates the handshake. Each subsequent test:
1. Calls `let (mut handler, dir) = setup().await;`
2. Calls a `ClientHandler` method
3. Asserts the returned value

Always wrap async test operations in `tokio::time::timeout` to prevent tests from hanging
if a response never arrives.

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
| `0x04` | `E_UNSUPPORTED` | Operation not supported by server |

#### `MOVE_REQ` semantics
- Destination exists and is a **directory** → `E_EXISTS`
- Destination exists and is a **file** → overwrite
- Source and destination are different types → `E_INVALID`

#### `RM_REQ` semantics
- On a file: simple unlink
- On a directory: recursive removal (equivalent to `rm -rf`)

### Transport layer
No `Transport` trait — instead, `AzpfsReader` and `AzpfsWriter` are blanket traits over
`AsyncRead`/`AsyncWrite` + bounds (`Unpin + Debug + Send + Sync + 'static`). Anything satisfying
those bounds (real `TcpStream` halves, `tokio::io::duplex` halves) works without any wrapper.

### `FsBackend` trait (`src/core/fs/`)
- `FsBackend` in `mod.rs` is a shared async trait with methods for all filesystem operations
- `DiskFs` in `disk.rs` implements it server-side: maps inode numbers → `PathBuf`s; inode entries are lazily populated on `lookup`
- `ClientHandler` in `handler.rs` also implements it client-side by issuing protocol requests

### Server (`src/core/server/`)
- `handle_client<R, W>(r: R, w: W)` — per-connection entry point, spawned by Tokio for each TCP client
- Internally: spawns a writer task draining an `mpsc::channel(32)`, and a reader loop that spawns
  a task per message calling `handle_msg(msg, tx, fs)`
- `handle_msg` in `handlers.rs` dispatches on all message types (all operations implemented)

### Client (`src/core/client/`)
- `ClientHandler<W: AzpfsWriter>` in `fs/handler.rs` — implements `FsBackend`; generic over the writer half
- `ClientHandler::new(r, w)` — spawns a `receive_loop` task, then performs the INIT handshake
- `receive_loop` routes incoming messages to per-request `mpsc::unbounded_channel` receivers keyed by request ID
- `FUSEFilesystem<F: FsBackend>` in `client/fuse.rs` wraps any `FsBackend` in a `Mutex` and implements `fuser::Filesystem`

## FUSE operations in scope

Core (required):
- `lookup`, `getattr`, `setattr`
- `readdir`, `mkdir`, `rmdir`
- `create`, `open`, `read`, `write`, `flush`, `release`
- `unlink`, `rename`

Stretch:
- `link`, `symlink`, `readlink` (inode links)
- `getlk`, `setlk` (POSIX flock)

## Development notes

Parts of the tooling and test suite were written with AI assistance. Commits carry
`Co-Authored-By` trailers where that applies. The protocol design, wire codec, and
architecture are my own.
