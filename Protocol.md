# AZPFS Protocol Specification

**Version 1.0**

## Table of contents

- [Session lifecycle](#session-lifecycle)
- [Message types & formatting](#message-types--formatting)
  - [ERROR](#error)
  - [INIT_REQ](#init_req)
  - [INIT_RES](#init_res)
  - [LOOKUP_REQ](#lookup_req)
  - [LOOKUP_RES](#lookup_res)
  - [GET_ATTR_REQ](#get_attr_req)
  - [FILE_ATTR_RES](#file_attr_res)
  - [SET_ATTR_REQ](#set_attr_req)
  - [SUCCESS_RES](#success_res)
  - [STATS_REQ](#stats_req)
  - [STATS_RES](#stats_res)
  - [CREATE_REQ](#create_req)
  - [READ_REQ](#read_req)
  - [READ_RES](#read_res)
  - [WRITE_REQ](#write_req)
  - [READDIR_REQ](#readdir_req)
  - [RM_REQ](#rm_req)
  - [MOVE_REQ](#move_req)
- [Error codes](#error-codes)

## Session lifecycle

A client connects to the server over TCP on a given port number. After the TCP socket is
established, the client must initiate the session via sending an `INIT_REQ` with a version of
`0x00`.

The server must reply with an `INIT_RES` with the accept flag set to true. The server must
respond to any further `INIT_REQ`'s with error `E_INVALID`. If the server receives an
`INIT_REQ` with an invalid (non-zero) version number, it must reply with `INIT_RES` with the
accept flag set to false.

If the server receives any other request before having accepted an `INIT_REQ`, it must reply to
such requests with `E_INVALID`. As such, clients must not send other requests until having
received an accepted `INIT_RES` from the server.

Clients should close connections via the normal TCP closing handshake, but may also do so via
sending a RST. Servers must end a session when its underlying TCP socket closes.

File open and release operations are no-ops over the protocol; file reads and writes are always
valid by inode without needing to be opened first.

## Message types & formatting

Most client requests are given a unique Request ID used to match server responses with them.
This ID is a 32-bit number that must be unique per pending request. The client must ensure that
no pending requests (that is, requests that the server has not finished responding to) share the
same ID. The client should not reuse IDs from finished requests until having exhausted the
32-bit number space and wrapping around.

In the below field tables, descriptions in **bold** indicate constant values for the given field.

Reserved fields must be set to 0.

All time-related fields must be given as Unix time.

### ERROR

Used by the server to return an error back to the client.

| Field | Offset (bits) | Size (bits) | Description |
|---|---|---|---|
| Message Type | 0 | 8 | **0x00** |
| Request ID | 8 | 32 | The ID of the client request that this is a response to. |
| Error Code | 40 | 8 | See the [Error codes](#error-codes) section below. |
| Message Len | 48 | 16 | The length, in bytes, of the error message |
| Message | 64 | … | An ASCII description of the error. Must be Message Len bytes long. |

### INIT_REQ

Used by the client to initiate a connection.

| Field | Offset (bits) | Size (bits) | Description |
|---|---|---|---|
| Message Type | 0 | 8 | **0x01** |
| Request ID | 8 | 32 | Allocated by client. See above. |
| Version | 40 | 4 | Version number to operate the session on. Should be `0x00`. |
| Reserved | 44 | 4 | **0** |

### INIT_RES

Used by the server to complete connection initiation.

| Field | Offset (bits) | Size (bits) | Description |
|---|---|---|---|
| Message Type | 0 | 8 | **0x02** |
| Request ID | 8 | 32 | The ID of the `INIT_REQ` this is a response to. |
| Accepted | 40 | 1 | **1** |
| Reserved | 41 | 7 | **0** |

### LOOKUP_REQ

Used by the client to look up a directory entry by filename and get its inode number for further
requests.

| Field | Offset (bits) | Size (bits) | Description |
|---|---|---|---|
| Message Type | 0 | 8 | **0x03** |
| Request ID | 8 | 32 | Allocated by client. See above. |
| Dir Inode | 40 | 64 | Inode number of the directory within which we're looking |
| Filename Len | 104 | 8 | Length of the filename in bytes |
| Filename | 112 | … | Bytes of the filename. Must be Filename Len bytes long. |

### LOOKUP_RES

Returns the inode number of the requested or created inode. Returned both from `LOOKUP_REQ` and
`CREATE_REQ`.

| Field | Offset (bits) | Size (bits) | Description |
|---|---|---|---|
| Message Type | 0 | 8 | **0x04** |
| Request ID | 8 | 32 | The ID of the request this is a response to |
| File Inode | 40 | 64 | The inode number of the file. |

### GET_ATTR_REQ

Used by the client to get the attributes of a file.

| Field | Offset (bits) | Size (bits) | Description |
|---|---|---|---|
| Message Type | 0 | 8 | **0x05** |
| Request ID | 8 | 32 | Allocated by client. See above. |
| File Inode | 40 | 64 | Inode number of the file whose attributes we want |

### FILE_ATTR_RES

Returns the attributes of the requested file.

| Field | Offset (bits) | Size (bits) | Description |
|---|---|---|---|
| Message Type | 0 | 8 | **0x06** |
| Request ID | 8 | 32 | The ID of the `GET_ATTR_REQ` this is a response to |
| File Type | 40 | 3 | The type of file. See the file type table below. |
| Reserved | 43 | 5 | **0** |
| Size | 48 | 64 | Size of the file in bytes |
| Blocks | 112 | 64 | Size of the file in 512-byte blocks. May be smaller than the actual file size if the file is compressed, for example. |
| Access Time | 176 | 64 | Time of last access |
| Modification Time | 240 | 64 | Time of last modification |
| Change Time | 304 | 64 | Time of last change |
| Permissions | 368 | 16 | Unix file mode |
| Hard Links | 384 | 32 | Number of hard links |
| UID | 416 | 32 | User ID |
| GID | 448 | 32 | Group ID |
| rdev | 480 | 32 | File's device ID. 0 for conventional files. |
| Block Size | 512 | 32 | Block size to be reported by `stat()` |

File type codes:

| Code | Type |
|---|---|
| `0x00` | Named pipe |
| `0x01` | Character device |
| `0x02` | Block device |
| `0x03` | Directory |
| `0x04` | Regular file |
| `0x05` | Symbolic link |
| `0x06` | Unix domain socket |

### SET_ATTR_REQ

Used by the client to set the attributes of a file. The server must reply with a `SUCCESS_RES`
or error as appropriate. This message's fields depend on the value of Field Mask as described
therein. Regardless of which fields are present, their order must be maintained.

| Field | Offset (bits) | Size (bits) | Description |
|---|---|---|---|
| Message Type | 0 | 8 | **0x07** |
| Request ID | 8 | 32 | Allocated by client. See above. |
| File Inode | 40 | 64 | Inode number of the file whose attributes we want to edit |
| Field Mask | 104 | 6 | Indicates to the server which fields we're modifying. Bit 0 corresponds to Size, bit 1 to Access Time, etc. Fields whose bit is set high are present in the message. Fields whose bit is set low must not be included in the message. |
| Reserved | 110 | 2 | **0** |
| Size | 112 | 64 | New size of the file in bytes. If size is reduced, data beyond the new size is discarded; if increased, a sparse "hole" is created. Only present if the appropriate Field Mask bit is set. |
| Access Time | ≥112 | 64 | New time of last access. Only present if the appropriate Field Mask bit is set. |
| Modification Time | ≥112 | 64 | New time of last modification. Only present if the appropriate Field Mask bit is set. |
| Permissions | ≥112 | 16 | New Unix file mode. Only present if the appropriate Field Mask bit is set. |
| UID | ≥112 | 32 | New user ID. Only present if the appropriate Field Mask bit is set. |
| GID | ≥112 | 32 | New group ID. Only present if the appropriate Field Mask bit is set. |

### SUCCESS_RES

Returned by the server upon a successful operation that does not return any data, such as file
writes, attribute updates, or inode removal.

| Field | Offset (bits) | Size (bits) | Description |
|---|---|---|---|
| Message Type | 0 | 8 | **0x08** |
| Request ID | 8 | 32 | The ID of the request this is a response to |

### STATS_REQ

Used by the client to get information about the filesystem.

| Field | Offset (bits) | Size (bits) | Description |
|---|---|---|---|
| Message Type | 0 | 8 | **0x09** |
| Request ID | 8 | 32 | Allocated by client. See above. |

### STATS_RES

Returns filesystem statistics from `statfs(2)`. Reflects the actual disk usage of the server.

| Field | Offset (bits) | Size (bits) | Description |
|---|---|---|---|
| Message Type | 0 | 8 | **0x0A** |
| Request ID | 8 | 32 | The ID of the `STATS_REQ` this is a response to |
| Block Size | 40 | 32 | Optimal transfer block size |
| Blocks | 72 | 64 | Total data blocks |
| Free Blocks | 136 | 64 | Number of free blocks |
| Available Blocks | 200 | 64 | Number of free blocks available to unprivileged users |
| Total Inodes | 264 | 64 | Total number of inodes |
| Free Inodes | 328 | 64 | Number of free inodes |
| Max Filename Len | 392 | 32 | Maximum filename length provided by the filesystem. |
| Fragment Size | 424 | 32 | Fragment size |

### CREATE_REQ

Used by the client to create a file or directory if it does not already exist. The server replies
with a `LOOKUP_RES`.

| Field | Offset (bits) | Size (bits) | Description |
|---|---|---|---|
| Message Type | 0 | 8 | **0x0B** |
| Request ID | 8 | 32 | Allocated by client. See above. |
| Dir Inode | 40 | 64 | Inode of the directory within which to create the file |
| Permissions | 104 | 16 | Unix file mode for the new file |
| Unix Flags | 120 | 32 | Open flags as passed by `open(2)` (e.g. `O_RDWR`, `O_TRUNC`). Ignored if Directory is true. |
| Directory | 152 | 1 | 1 if the inode to be created is a directory, 0 otherwise. |
| Reserved | 153 | 7 | **0** |
| Filename Length | 160 | 8 | The length of the filename in bytes |
| Filename | 168 | … | The bytes of the filename. Must be Filename Length bytes. |

### READ_REQ

A request from the client to read data from the given inode.

| Field | Offset (bits) | Size (bits) | Description |
|---|---|---|---|
| Message Type | 0 | 8 | **0x0C** |
| Request ID | 8 | 32 | Allocated by client. See above. |
| Inode | 40 | 64 | Inode of the file from which to read |
| Offset | 104 | 64 | Offset into the file from which to read |
| Length | 168 | 64 | Number of the requested bytes. The server must return up to this number of bytes but may return less (e.g. due to EOF). |

### READ_RES

A chunk of read data issued by the server in response to a `READ_REQ` or `READDIR_REQ`. Depending
on the length of the returned data, multiple of these may be required to serve a single request.
Chunks may be returned out of order, and clients must be able to assemble out-of-order `READ_RES`
chunks.

| Field | Offset (bits) | Size (bits) | Description |
|---|---|---|---|
| Message Type | 0 | 8 | **0x0D** |
| Request ID | 8 | 32 | The ID of the request that this is a response to |
| Total Length | 40 | 64 | The total number of bytes returned, over all chunks, from the request |
| Chunk Length | 104 | 16 | The number of bytes in this chunk. |
| Chunk Offset | 120 | 64 | The offset into the total number of read bytes where this chunk starts. For instance, a value of 0 would indicate that this is the first chunk, while a value of (Total Length – Chunk Length) would indicate that this is the final chunk. |
| Data | 184 | … | The data of this chunk. Must be Chunk Length bytes long. |

If the server is replying to a `READDIR_REQ`, the payload data is a series of packed directory
entries of the following format.

| Field | Offset (bits) | Size (bits) | Description |
|---|---|---|---|
| Inode Number | 0 | 64 | The inode number of the directory entry. |
| File Type | 64 | 8 | The file type of the directory entry, as defined in the section on [FILE_ATTR_RES](#file_attr_res). |
| Filename Length | 72 | 8 | The length of the filename, in bytes |
| Filename | 80 | … | The filename of the directory entry. Must be Filename Length bytes long. |

### WRITE_REQ

A request from the client to write data to the given inode. The server must write all data in the
`WRITE_REQ` and return a `SUCCESS_RES`. If the server was unable to write all the data, the
operation must instead fail, without writing anything, and return an appropriate error.

| Field | Offset (bits) | Size (bits) | Description |
|---|---|---|---|
| Message Type | 0 | 8 | **0x0E** |
| Request ID | 8 | 32 | Allocated by client. See above. |
| Inode | 40 | 64 | Inode of the file to write to |
| Offset | 104 | 64 | Offset into the file from which to begin writing |
| Length | 168 | 32 | Number of bytes to write |
| Data | 200 | … | The data to be written. Must be Length bytes long. |

### READDIR_REQ

Used by the client to read the entries of a given directory. The server must return all directory
entries via a `READ_RES`, as defined in the section on that message type.

| Field | Offset (bits) | Size (bits) | Description |
|---|---|---|---|
| Message Type | 0 | 8 | **0x0F** |
| Request ID | 8 | 32 | Allocated by client. See above. |
| Inode | 40 | 64 | The inode number of the directory. |

### RM_REQ

Used by the client to remove an inode, corresponding to the `unlink(2)` and `rmdir(2)` syscalls.
If invoked on a file, the server must simply remove it; if invoked on a directory, the server must
remove it and all its contents recursively (as in `rm -rf`). The server must reply with a
`SUCCESS_RES` or error as appropriate.

| Field | Offset (bits) | Size (bits) | Description |
|---|---|---|---|
| Message Type | 0 | 8 | **0x10** |
| Request ID | 8 | 32 | Allocated by client. See above. |
| Inode | 40 | 64 | The inode number of the inode to remove. |

### MOVE_REQ

Used by the client to move or rename an inode. The server must reply with a `SUCCESS_RES` or error
as appropriate. If the destination already exists and is a directory, the server must reply with an
`E_EXISTS`. If it exists and is a file, the server must overwrite it. If the source and destination
are of different types (e.g. a file vs directory), the server must reply with an `E_INVALID`.

| Field | Offset (bits) | Size (bits) | Description |
|---|---|---|---|
| Message Type | 0 | 8 | **0x11** |
| Request ID | 8 | 32 | Allocated by client. See above. |
| Inode | 40 | 64 | The inode number of the inode to move. |
| Destination Dir | 104 | 64 | The inode number of the directory to which the inode should be moved. |
| Destination Filename Length | 168 | 8 | The length of the destination filename in bytes. |
| Destination Filename | 176 | … | The filename within the destination directory to which the inode should be moved. Must be Destination Filename Length bytes long. |

## Error codes

The following error codes may be responded by the server after a client request:

| Error code | Meaning |
|---|---|
| `0x00` | `E_INTERNAL`. The server encountered a miscellaneous internal error. See the error message for more information. |
| `0x01` | `E_INVALID`. The given request is malformed or otherwise cannot be processed due to a client error. |
| `0x02` | `E_NOTFOUND`. The requested inode doesn't exist |
| `0x03` | `E_EXISTS`. An inode already exists at the requested destination path of a creation or renaming operation. |
| `0x04` | `E_UNSUPPORTED`. The server does not support the given operation. |
