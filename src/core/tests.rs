use crate::{
    client::ClientHandler,
    fs::{DiskFs, FileType, FsBackend},
    server::handle_client,
};
use std::{
    io::ErrorKind,
    os::unix::fs::MetadataExt,
    path::Path,
    sync::Arc,
    time::{Duration, SystemTime},
};
use tempfile::TempDir;
use tokio::{sync::Mutex, time::timeout};

const TIMEOUT: Duration = Duration::from_secs(5);

// The writer half is either a duplex stream or a TCP OwnedWriteHalf; box them
// to a common type so setup() has a single return type.
type BoxWriter = Box<dyn crate::AzpfsWriter + Send + Sync>;

/// Wire a `ClientHandler` to a `handle_client` task.
///
/// By default uses in-memory duplex channels. Set `AZPFS_TCP=1` to use a
/// real loopback TCP connection instead, making traffic visible in Wireshark
/// (capture on `lo`).
///
/// Also returns the `TempDir` so the caller keeps it alive for the test's
/// duration (dropping it would delete the backing directory).
async fn setup() -> (ClientHandler<BoxWriter>, TempDir) {
    let dir = TempDir::new().expect("failed to create temp dir");
    let fs = Arc::new(Mutex::new(DiskFs::new(dir.path().to_path_buf())));

    let (client_r, client_w): (Box<dyn crate::AzpfsReader>, BoxWriter) =
        if let Ok(port) = std::env::var("AZPFS_TCP") {
            let addr = format!("127.0.0.1:{port}");
            let listener =
                tokio::net::TcpListener::bind(&addr).await.unwrap();

            let fs = Arc::clone(&fs);
            tokio::spawn(async move {
                let (stream, _) = listener.accept().await.unwrap();
                let (r, w) = stream.into_split();
                handle_client(r, w, fs).await;
            });

            let stream =
                tokio::net::TcpStream::connect(&addr).await.unwrap();
            let (r, w) = stream.into_split();
            (Box::new(r), Box::new(w))
        } else {
            let (client_reader, server_writer) = tokio::io::duplex(4096);
            let (server_reader, client_writer) = tokio::io::duplex(4096);
            tokio::spawn(handle_client(server_reader, server_writer, fs));
            (Box::new(client_reader), Box::new(client_writer))
        };

    let handler = timeout(TIMEOUT, ClientHandler::new(client_r, client_w))
        .await
        .expect("setup timed out")
        .expect("ClientHandler::new failed");
    (handler, dir)
}

/// Wraps a future in a 5-second timeout, panicking if it exceeds it.
async fn t<F: std::future::Future>(f: F) -> F::Output {
    timeout(TIMEOUT, f).await.expect("timed out")
}

// ── Init ─────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_init_handshake() {
    // ClientHandler::new performs the INIT handshake internally.
    // Successful construction is the assertion.
    let (_handler, _dir) = setup().await;
}

// ── Lookup ───────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_lookup_existing_file() {
    let (mut handler, dir) = setup().await;

    // Create a real file so DiskFs can stat it.
    std::fs::write(dir.path().join("hello.txt"), b"hi").unwrap();
    let expected_ino = std::fs::metadata(dir.path().join("hello.txt"))
        .unwrap()
        .ino();

    let ino = t(handler.lookup(super::ROOT_INODE, Path::new("hello.txt")))
        .await
        .expect("lookup failed");

    assert_eq!(ino, expected_ino);
}

#[tokio::test]
async fn test_lookup_existing_dir() {
    let (mut handler, dir) = setup().await;

    std::fs::create_dir(dir.path().join("subdir")).unwrap();
    let expected_ino =
        std::fs::metadata(dir.path().join("subdir")).unwrap().ino();

    let ino = t(handler.lookup(super::ROOT_INODE, Path::new("subdir")))
        .await
        .expect("lookup failed");

    assert_eq!(ino, expected_ino);
}

#[tokio::test]
async fn test_lookup_nonexistent() {
    let (mut handler, _dir) = setup().await;

    let err = t(handler.lookup(super::ROOT_INODE, Path::new("does_not_exist")))
        .await
        .expect_err("expected NotFound error");

    assert_eq!(err.kind(), ErrorKind::NotFound);
}

#[tokio::test]
async fn test_lookup_nested() {
    let (mut handler, dir) = setup().await;

    // Create subdir/child.txt on disk.
    std::fs::create_dir(dir.path().join("subdir")).unwrap();
    std::fs::write(dir.path().join("subdir/child.txt"), b"data").unwrap();

    let expected_child_ino =
        std::fs::metadata(dir.path().join("subdir/child.txt"))
            .unwrap()
            .ino();

    // Look up the subdir first to register it in the server's inode_map.
    let subdir_ino =
        t(handler.lookup(super::ROOT_INODE, Path::new("subdir")))
            .await
            .expect("lookup subdir failed");

    // Now look up the child using subdir_ino as parent.
    let child_ino = t(handler.lookup(subdir_ino, Path::new("child.txt")))
        .await
        .expect("lookup child failed");

    assert_eq!(child_ino, expected_child_ino);
}

// ── Get attributes ───────────────────────────────────────────────────────────

#[tokio::test]
async fn test_get_attr_root() {
    let (mut handler, _dir) = setup().await;

    let attr = t(handler.get_attr(super::ROOT_INODE))
        .await
        .expect("get_attr root failed");

    assert_eq!(attr.file_type, FileType::Directory);
}

#[tokio::test]
async fn test_get_attr_file() {
    let (mut handler, dir) = setup().await;

    let data = b"hello world";
    std::fs::write(dir.path().join("file.txt"), data).unwrap();

    let ino = t(handler.lookup(super::ROOT_INODE, Path::new("file.txt")))
        .await
        .expect("lookup failed");

    let attr = t(handler.get_attr(ino)).await.expect("get_attr failed");

    assert_eq!(attr.file_type, FileType::RegularFile);
    assert_eq!(attr.size, data.len() as u64);
}

#[tokio::test]
async fn test_get_attr_nonexistent_inode() {
    let (mut handler, _dir) = setup().await;

    // Use a bogus inode that was never registered in the inode_map.
    let err = t(handler.get_attr(999_999_999))
        .await
        .expect_err("expected error for unknown inode");

    // Could be NotFound or Other depending on implementation; just assert it's
    // an error.
    let _ = err.kind();
}

// ── Set attributes ───────────────────────────────────────────────────────────

#[tokio::test]
async fn test_set_attr_permissions() {
    let (mut handler, dir) = setup().await;

    std::fs::write(dir.path().join("chmod_me.txt"), b"data").unwrap();

    let ino =
        t(handler.lookup(super::ROOT_INODE, Path::new("chmod_me.txt")))
            .await
            .expect("lookup failed");

    t(handler.set_attr(ino, None, None, None, Some(0o600), None, None))
        .await
        .expect("set_attr failed");

    // Verify the mode was actually changed on disk (lower 9 bits).
    let mode = std::fs::metadata(dir.path().join("chmod_me.txt"))
        .unwrap()
        .mode();
    assert_eq!(mode & 0o777, 0o600);
}

#[tokio::test]
async fn test_set_attr_size_truncate() {
    let (mut handler, dir) = setup().await;

    std::fs::write(dir.path().join("trunc.txt"), b"some data here").unwrap();

    let ino = t(handler.lookup(super::ROOT_INODE, Path::new("trunc.txt")))
        .await
        .expect("lookup failed");

    t(handler.set_attr(ino, Some(0), None, None, None, None, None))
        .await
        .expect("set_attr size=0 failed");

    let len = std::fs::metadata(dir.path().join("trunc.txt")).unwrap().len();
    assert_eq!(len, 0);
}

#[tokio::test]
async fn test_set_attr_mtime_only() {
    let (mut handler, dir) = setup().await;

    let content = b"unchanged content";
    std::fs::write(dir.path().join("mtime.txt"), content).unwrap();

    let ino = t(handler.lookup(super::ROOT_INODE, Path::new("mtime.txt")))
        .await
        .expect("lookup failed");

    // Pick a specific mtime to set (Jan 1 2000).
    let new_mtime =
        SystemTime::UNIX_EPOCH + Duration::from_secs(946_684_800);

    t(handler.set_attr(ino, None, None, Some(new_mtime), None, None, None))
        .await
        .expect("set_attr mtime failed");

    // Size should be unchanged.
    let len = std::fs::metadata(dir.path().join("mtime.txt")).unwrap().len();
    assert_eq!(len, content.len() as u64);
}

// ── Stats ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_stats() {
    let (mut handler, _dir) = setup().await;

    let err = t(handler.stats()).await.expect_err("expected stats to fail");
    assert_eq!(err.kind(), ErrorKind::Unsupported);
}

// ── Create file / dir ────────────────────────────────────────────────────────

#[tokio::test]
async fn test_create_file() {
    let (mut handler, dir) = setup().await;

    let ino =
        t(handler.create_file(super::ROOT_INODE, 0o644, 0, Path::new("new.txt")))
            .await
            .expect("create_file failed");

    assert!(ino > 0);
    let meta = std::fs::metadata(dir.path().join("new.txt")).unwrap();
    assert!(meta.is_file());
    assert_eq!(meta.mode() & 0o777, 0o644);

    let attr = t(handler.get_attr(ino)).await.expect("get_attr failed");
    assert_eq!(attr.file_type, FileType::RegularFile);
    assert_eq!(attr.permissions as u32 & 0o777, 0o644);
}

#[tokio::test]
async fn test_create_dir() {
    let (mut handler, dir) = setup().await;

    let ino =
        t(handler.create_dir(super::ROOT_INODE, 0o755, Path::new("newdir")))
            .await
            .expect("create_dir failed");

    assert!(ino > 0);
    let meta = std::fs::metadata(dir.path().join("newdir")).unwrap();
    assert!(meta.is_dir());
    assert_eq!(meta.mode() & 0o777, 0o755);

    let attr = t(handler.get_attr(ino)).await.expect("get_attr failed");
    assert_eq!(attr.file_type, FileType::Directory);
    assert_eq!(attr.permissions as u32 & 0o777, 0o755);
}

#[tokio::test]
async fn test_create_file_then_lookup() {
    let (mut handler, _dir) = setup().await;

    let created_ino =
        t(handler.create_file(super::ROOT_INODE, 0o644, 0, Path::new("roundtrip.txt")))
            .await
            .expect("create_file failed");

    let looked_up_ino =
        t(handler.lookup(super::ROOT_INODE, Path::new("roundtrip.txt")))
            .await
            .expect("lookup failed");

    assert_eq!(created_ino, looked_up_ino);
}

// ── Read & write ─────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_read_file() {
    let (mut handler, dir) = setup().await;

    let data = b"hello from disk";
    std::fs::write(dir.path().join("read_me.txt"), data).unwrap();

    let ino = t(handler.lookup(super::ROOT_INODE, Path::new("read_me.txt")))
        .await
        .expect("lookup failed");

    let result = t(handler.read(ino, 0, data.len() as u64))
        .await
        .expect("read failed");

    assert_eq!(result, data);
}

#[tokio::test]
async fn test_read_at_offset() {
    let (mut handler, dir) = setup().await;

    let data = b"0123456789";
    std::fs::write(dir.path().join("offset.txt"), data).unwrap();

    let ino = t(handler.lookup(super::ROOT_INODE, Path::new("offset.txt")))
        .await
        .expect("lookup failed");

    // Read bytes [3, 7).
    let result = t(handler.read(ino, 3, 4)).await.expect("read failed");

    assert_eq!(result, b"3456");
}

#[tokio::test]
async fn test_read_empty_file() {
    let (mut handler, dir) = setup().await;

    std::fs::write(dir.path().join("empty.txt"), b"").unwrap();

    let ino = t(handler.lookup(super::ROOT_INODE, Path::new("empty.txt")))
        .await
        .expect("lookup failed");

    let result = t(handler.read(ino, 0, 1024)).await.expect("read failed");

    assert!(result.is_empty());
}

#[tokio::test]
async fn test_write_file() {
    let (mut handler, dir) = setup().await;

    // Create the file on disk first so lookup can register it.
    std::fs::write(dir.path().join("write_me.txt"), b"").unwrap();

    let ino =
        t(handler.lookup(super::ROOT_INODE, Path::new("write_me.txt")))
            .await
            .expect("lookup failed");

    let payload = b"written by handler";
    t(handler.write(ino, 0, payload)).await.expect("write failed");

    let on_disk = std::fs::read(dir.path().join("write_me.txt")).unwrap();
    assert_eq!(on_disk, payload);
}

#[tokio::test]
async fn test_write_at_offset() {
    let (mut handler, dir) = setup().await;

    // Pre-fill with zeros.
    std::fs::write(dir.path().join("sparse.txt"), b"\x00\x00\x00\x00\x00")
        .unwrap();

    let ino = t(handler.lookup(super::ROOT_INODE, Path::new("sparse.txt")))
        .await
        .expect("lookup failed");

    t(handler.write(ino, 2, b"AB")).await.expect("write failed");

    let on_disk = std::fs::read(dir.path().join("sparse.txt")).unwrap();
    assert_eq!(&on_disk[2..4], b"AB");
}

#[tokio::test]
async fn test_write_then_read_roundtrip() {
    let (mut handler, dir) = setup().await;

    std::fs::write(dir.path().join("roundtrip.txt"), b"").unwrap();

    let ino =
        t(handler.lookup(super::ROOT_INODE, Path::new("roundtrip.txt")))
            .await
            .expect("lookup failed");

    let payload = b"roundtrip payload";
    t(handler.write(ino, 0, payload)).await.expect("write failed");

    let result = t(handler.read(ino, 0, payload.len() as u64))
        .await
        .expect("read failed");

    assert_eq!(result, payload);
}

// ── Read directory ───────────────────────────────────────────────────────────

#[tokio::test]
async fn test_read_dir_empty_root() {
    let (mut handler, _dir) = setup().await;

    let entries = t(handler.read_dir(super::ROOT_INODE))
        .await
        .expect("read_dir failed");

    assert_eq!(entries.len(), 2, "expected exactly . and .. entries, got {entries:?}");

    let names: Vec<&str> = entries
        .iter()
        .filter_map(|e| std::str::from_utf8(&e.filename).ok())
        .collect();

    assert!(names.contains(&"."), "missing . entry");
    assert!(names.contains(&".."), "missing .. entry");
}

#[tokio::test]
async fn test_read_dir_with_entries() {
    let (mut handler, dir) = setup().await;

    std::fs::write(dir.path().join("alpha.txt"), b"").unwrap();
    std::fs::write(dir.path().join("beta.txt"), b"").unwrap();
    std::fs::create_dir(dir.path().join("subdir")).unwrap();

    let entries = t(handler.read_dir(super::ROOT_INODE))
        .await
        .expect("read_dir failed");

    let names: Vec<&str> = entries
        .iter()
        .filter_map(|e| std::str::from_utf8(&e.filename).ok())
        .collect();

    assert!(names.contains(&"alpha.txt"), "missing alpha.txt in {names:?}");
    assert!(names.contains(&"beta.txt"), "missing beta.txt in {names:?}");
    assert!(names.contains(&"subdir"), "missing subdir in {names:?}");
}

#[tokio::test]
async fn test_read_dir_entry_types() {
    let (mut handler, dir) = setup().await;

    std::fs::write(dir.path().join("file.txt"), b"").unwrap();
    std::fs::create_dir(dir.path().join("adir")).unwrap();

    let entries = t(handler.read_dir(super::ROOT_INODE))
        .await
        .expect("read_dir failed");

    for e in &entries {
        let name = std::str::from_utf8(&e.filename).unwrap_or("");
        match name {
            "file.txt" => {
                assert_eq!(e.file_type, FileType::RegularFile)
            }
            "adir" => assert_eq!(e.file_type, FileType::Directory),
            _ => {} // . and ..
        }
    }
}

#[tokio::test]
async fn test_read_dir_on_file_fails() {
    let (mut handler, dir) = setup().await;

    std::fs::write(dir.path().join("notadir.txt"), b"data").unwrap();

    let ino = t(handler.lookup(super::ROOT_INODE, Path::new("notadir.txt")))
        .await
        .expect("lookup failed");

    t(handler.read_dir(ino))
        .await
        .expect_err("expected error when calling read_dir on a file");
}

// ── Remove ───────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_remove_file() {
    let (mut handler, dir) = setup().await;

    std::fs::write(dir.path().join("gone.txt"), b"bye").unwrap();

    let ino = t(handler.lookup(super::ROOT_INODE, Path::new("gone.txt")))
        .await
        .expect("lookup failed");

    t(handler.remove(ino)).await.expect("remove failed");

    assert!(!dir.path().join("gone.txt").exists());
}

#[tokio::test]
async fn test_remove_file_then_lookup_fails() {
    let (mut handler, dir) = setup().await;

    std::fs::write(dir.path().join("poof.txt"), b"").unwrap();

    let ino = t(handler.lookup(super::ROOT_INODE, Path::new("poof.txt")))
        .await
        .expect("lookup failed");

    t(handler.remove(ino)).await.expect("remove failed");

    let err = t(handler.lookup(super::ROOT_INODE, Path::new("poof.txt")))
        .await
        .expect_err("expected NotFound after remove");

    assert_eq!(err.kind(), ErrorKind::NotFound);
}

#[tokio::test]
async fn test_remove_dir_recursive() {
    let (mut handler, dir) = setup().await;

    // Create a dir with nested content.
    std::fs::create_dir(dir.path().join("todelete")).unwrap();
    std::fs::write(dir.path().join("todelete/inner.txt"), b"data").unwrap();
    std::fs::create_dir(dir.path().join("todelete/nested")).unwrap();
    std::fs::write(
        dir.path().join("todelete/nested/deep.txt"),
        b"deep",
    )
    .unwrap();

    let ino = t(handler.lookup(super::ROOT_INODE, Path::new("todelete")))
        .await
        .expect("lookup failed");

    t(handler.remove(ino)).await.expect("remove dir failed");

    assert!(!dir.path().join("todelete").exists());
}

#[tokio::test]
async fn test_remove_nonexistent_inode() {
    let (mut handler, _dir) = setup().await;

    // A bogus inode that was never registered.
    let err = t(handler.remove(888_888_888))
        .await
        .expect_err("expected error for unknown inode");

    let _ = err.kind(); // just assert it's an error
}

// ── Rename ───────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_rename_file() {
    let (mut handler, dir) = setup().await;

    std::fs::write(dir.path().join("before.txt"), b"contents").unwrap();

    let ino = t(handler.lookup(super::ROOT_INODE, Path::new("before.txt")))
        .await
        .expect("lookup failed");

    t(handler.rename(ino, super::ROOT_INODE, Path::new("after.txt")))
        .await
        .expect("rename failed");

    assert!(!dir.path().join("before.txt").exists());
    assert!(dir.path().join("after.txt").exists());
    assert_eq!(
        std::fs::read(dir.path().join("after.txt")).unwrap(),
        b"contents"
    );
}

#[tokio::test]
async fn test_rename_overwrite_file() {
    let (mut handler, dir) = setup().await;

    std::fs::write(dir.path().join("src.txt"), b"src data").unwrap();
    std::fs::write(dir.path().join("dst.txt"), b"old dst").unwrap();

    let src_ino = t(handler.lookup(super::ROOT_INODE, Path::new("src.txt")))
        .await
        .expect("lookup src failed");

    t(handler.rename(src_ino, super::ROOT_INODE, Path::new("dst.txt")))
        .await
        .expect("rename failed");

    assert!(!dir.path().join("src.txt").exists());
    assert_eq!(
        std::fs::read(dir.path().join("dst.txt")).unwrap(),
        b"src data"
    );
}

#[tokio::test]
async fn test_rename_file_onto_dir_errors() {
    let (mut handler, dir) = setup().await;

    std::fs::write(dir.path().join("thefile.txt"), b"data").unwrap();
    std::fs::create_dir(dir.path().join("thedir")).unwrap();

    let file_ino =
        t(handler.lookup(super::ROOT_INODE, Path::new("thefile.txt")))
            .await
            .expect("lookup file failed");

    // Per spec: destination exists and is a directory → E_EXISTS
    let err =
        t(handler.rename(file_ino, super::ROOT_INODE, Path::new("thedir")))
            .await
            .expect_err("expected error when renaming file onto dir");

    // ErrorKind::AlreadyExists or Other; just assert it's an error.
    let _ = err.kind();
}

#[tokio::test]
async fn test_rename_into_subdir() {
    let (mut handler, dir) = setup().await;

    std::fs::write(dir.path().join("moveme.txt"), b"moving").unwrap();
    std::fs::create_dir(dir.path().join("dest_dir")).unwrap();

    let file_ino =
        t(handler.lookup(super::ROOT_INODE, Path::new("moveme.txt")))
            .await
            .expect("lookup file failed");
    let dest_dir_ino =
        t(handler.lookup(super::ROOT_INODE, Path::new("dest_dir")))
            .await
            .expect("lookup dest_dir failed");

    t(handler.rename(file_ino, dest_dir_ino, Path::new("moveme.txt")))
        .await
        .expect("rename into subdir failed");

    assert!(!dir.path().join("moveme.txt").exists());
    assert_eq!(
        std::fs::read(dir.path().join("dest_dir/moveme.txt")).unwrap(),
        b"moving"
    );
}

// ── Read/write combos ───────────────────────────────────────────────────────

#[tokio::test]
async fn test_write_overlapping_then_read() {
    let (mut handler, dir) = setup().await;

    std::fs::write(dir.path().join("overlap.txt"), b"").unwrap();
    let ino = t(handler.lookup(super::ROOT_INODE, Path::new("overlap.txt")))
        .await
        .expect("lookup failed");

    // First write: "AAAAAAAAAA" at offset 0
    t(handler.write(ino, 0, &[b'A'; 10]))
        .await
        .expect("write 1 failed");

    // Second write: "BBBBB" at offset 5 — overlaps bytes [5..10)
    t(handler.write(ino, 5, &[b'B'; 5]))
        .await
        .expect("write 2 failed");

    let data = t(handler.read(ino, 0, 10)).await.expect("read failed");
    assert_eq!(&data, b"AAAAABBBBB");
}

#[tokio::test]
async fn test_write_gap_then_read_full() {
    let (mut handler, dir) = setup().await;

    std::fs::write(dir.path().join("gap.txt"), b"").unwrap();
    let ino = t(handler.lookup(super::ROOT_INODE, Path::new("gap.txt")))
        .await
        .expect("lookup failed");

    // Write at offset 0
    t(handler.write(ino, 0, b"HEAD")).await.expect("write 1 failed");

    // Write at offset 8 — leaves a gap of 4 zero bytes at [4..8)
    t(handler.write(ino, 8, b"TAIL")).await.expect("write 2 failed");

    let data = t(handler.read(ino, 0, 12)).await.expect("read failed");
    assert_eq!(&data[..4], b"HEAD");
    assert_eq!(&data[4..8], &[0u8; 4]); // gap should be zeros
    assert_eq!(&data[8..], b"TAIL");
}

#[tokio::test]
async fn test_write_middle_preserves_surrounding() {
    let (mut handler, dir) = setup().await;

    std::fs::write(dir.path().join("mid.txt"), b"0123456789").unwrap();
    let ino = t(handler.lookup(super::ROOT_INODE, Path::new("mid.txt")))
        .await
        .expect("lookup failed");

    // Overwrite only bytes [3..6) with "XYZ"
    t(handler.write(ino, 3, b"XYZ")).await.expect("write failed");

    let data = t(handler.read(ino, 0, 10)).await.expect("read failed");
    assert_eq!(&data, b"012XYZ6789");
}

#[tokio::test]
async fn test_write_extend_file() {
    let (mut handler, dir) = setup().await;

    std::fs::write(dir.path().join("extend.txt"), b"short").unwrap();
    let ino = t(handler.lookup(super::ROOT_INODE, Path::new("extend.txt")))
        .await
        .expect("lookup failed");

    // Write past the current end of the file
    t(handler.write(ino, 5, b" and now longer"))
        .await
        .expect("write failed");

    let data = t(handler.read(ino, 0, 20)).await.expect("read failed");
    assert_eq!(&data, b"short and now longer");
}

#[tokio::test]
async fn test_read_past_eof_returns_short() {
    let (mut handler, dir) = setup().await;

    std::fs::write(dir.path().join("small.txt"), b"abc").unwrap();
    let ino = t(handler.lookup(super::ROOT_INODE, Path::new("small.txt")))
        .await
        .expect("lookup failed");

    // Request 1024 bytes from a 3-byte file
    let data = t(handler.read(ino, 0, 1024)).await.expect("read failed");
    assert_eq!(&data, b"abc");
}

#[tokio::test]
async fn test_read_at_eof_returns_empty() {
    let (mut handler, dir) = setup().await;

    std::fs::write(dir.path().join("eof.txt"), b"abc").unwrap();
    let ino = t(handler.lookup(super::ROOT_INODE, Path::new("eof.txt")))
        .await
        .expect("lookup failed");

    // Read starting exactly at the end
    let data = t(handler.read(ino, 3, 10)).await.expect("read failed");
    assert!(data.is_empty());
}

#[tokio::test]
async fn test_truncate_then_write_then_read() {
    let (mut handler, dir) = setup().await;

    std::fs::write(dir.path().join("trw.txt"), b"old content here").unwrap();
    let ino = t(handler.lookup(super::ROOT_INODE, Path::new("trw.txt")))
        .await
        .expect("lookup failed");

    // Truncate to 0
    t(handler.set_attr(ino, Some(0), None, None, None, None, None))
        .await
        .expect("truncate failed");

    // Write new content
    t(handler.write(ino, 0, b"fresh")).await.expect("write failed");

    let data = t(handler.read(ino, 0, 100)).await.expect("read failed");
    assert_eq!(&data, b"fresh");
}

#[tokio::test]
async fn test_multiple_sequential_writes_then_full_read() {
    let (mut handler, dir) = setup().await;

    std::fs::write(dir.path().join("seq.txt"), b"").unwrap();
    let ino = t(handler.lookup(super::ROOT_INODE, Path::new("seq.txt")))
        .await
        .expect("lookup failed");

    // Build up "Hello, world!" one piece at a time
    t(handler.write(ino, 0, b"Hello")).await.unwrap();
    t(handler.write(ino, 5, b", ")).await.unwrap();
    t(handler.write(ino, 7, b"world!")).await.unwrap();

    let data = t(handler.read(ino, 0, 13)).await.expect("read failed");
    assert_eq!(&data, b"Hello, world!");
}

#[tokio::test]
async fn test_create_write_read_delete_cycle() {
    let (mut handler, _dir) = setup().await;

    let ino = t(handler.create_file(
        super::ROOT_INODE,
        0o644,
        0,
        Path::new("lifecycle.txt"),
    ))
    .await
    .expect("create failed");

    t(handler.write(ino, 0, b"lifecycle data"))
        .await
        .expect("write failed");

    let data = t(handler.read(ino, 0, 100)).await.expect("read failed");
    assert_eq!(&data, b"lifecycle data");

    t(handler.remove(ino)).await.expect("remove failed");

    // Reading after remove should fail
    let err = t(handler.read(ino, 0, 1))
        .await
        .expect_err("expected error after remove");
    let _ = err.kind();
}

// ── Multi-client ────────────────────────────────────────────────────────────

/// Wire two independent clients to the same server-side DiskFs.
async fn setup_multi() -> (
    ClientHandler<BoxWriter>,
    ClientHandler<BoxWriter>,
    TempDir,
) {
    let dir = TempDir::new().expect("failed to create temp dir");
    let fs = Arc::new(Mutex::new(DiskFs::new(dir.path().to_path_buf())));

    let make_client = |fs: Arc<Mutex<DiskFs>>| async move {
        let (cr, sw) = tokio::io::duplex(4096);
        let (sr, cw) = tokio::io::duplex(4096);
        tokio::spawn(handle_client(sr, sw, fs));
        let cr: Box<dyn crate::AzpfsReader> = Box::new(cr);
        let cw: BoxWriter = Box::new(cw);
        timeout(TIMEOUT, ClientHandler::new(cr, cw))
            .await
            .expect("setup timed out")
            .expect("ClientHandler::new failed")
    };

    let c1 = make_client(Arc::clone(&fs)).await;
    let c2 = make_client(Arc::clone(&fs)).await;
    (c1, c2, dir)
}

#[tokio::test]
async fn test_multi_client_write_visible_to_other() {
    let (mut c1, mut c2, dir) = setup_multi().await;

    // Client 1 creates a file and writes to it
    std::fs::write(dir.path().join("shared.txt"), b"").unwrap();
    let ino1 = t(c1.lookup(super::ROOT_INODE, Path::new("shared.txt")))
        .await
        .expect("c1 lookup failed");

    t(c1.write(ino1, 0, b"from client 1"))
        .await
        .expect("c1 write failed");

    // Client 2 looks up the same file and reads it
    let ino2 = t(c2.lookup(super::ROOT_INODE, Path::new("shared.txt")))
        .await
        .expect("c2 lookup failed");

    let data = t(c2.read(ino2, 0, 100)).await.expect("c2 read failed");
    assert_eq!(&data, b"from client 1");
}

#[tokio::test]
async fn test_multi_client_both_write_same_file() {
    let (mut c1, mut c2, dir) = setup_multi().await;

    std::fs::write(dir.path().join("clobber.txt"), b"\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00").unwrap();

    let ino1 = t(c1.lookup(super::ROOT_INODE, Path::new("clobber.txt")))
        .await
        .unwrap();
    let ino2 = t(c2.lookup(super::ROOT_INODE, Path::new("clobber.txt")))
        .await
        .unwrap();

    // Both clients write to different regions of the same file
    t(c1.write(ino1, 0, b"AAAAA")).await.unwrap();
    t(c2.write(ino2, 5, b"BBBBB")).await.unwrap();

    // Either client should see the combined result
    let data = t(c1.read(ino1, 0, 10)).await.expect("read failed");
    assert_eq!(&data, b"AAAAABBBBB");
}

#[tokio::test]
async fn test_multi_client_create_visible_in_readdir() {
    let (mut c1, mut c2, _dir) = setup_multi().await;

    // Client 1 creates a file
    t(c1.create_file(super::ROOT_INODE, 0o644, 0, Path::new("c1_file.txt")))
        .await
        .expect("c1 create failed");

    // Client 2 creates a different file
    t(c2.create_file(super::ROOT_INODE, 0o644, 0, Path::new("c2_file.txt")))
        .await
        .expect("c2 create failed");

    // Client 1 reads the directory and should see both files
    let entries = t(c1.read_dir(super::ROOT_INODE))
        .await
        .expect("readdir failed");

    let names: Vec<&str> = entries
        .iter()
        .filter_map(|e| std::str::from_utf8(&e.filename).ok())
        .collect();

    assert!(names.contains(&"c1_file.txt"), "missing c1_file.txt in {names:?}");
    assert!(names.contains(&"c2_file.txt"), "missing c2_file.txt in {names:?}");
}

#[tokio::test]
async fn test_multi_client_remove_reflected() {
    let (mut c1, mut c2, dir) = setup_multi().await;

    std::fs::write(dir.path().join("doomed.txt"), b"bye").unwrap();

    let ino1 = t(c1.lookup(super::ROOT_INODE, Path::new("doomed.txt")))
        .await
        .unwrap();

    // Client 1 removes the file
    t(c1.remove(ino1)).await.expect("c1 remove failed");

    // Client 2 should not be able to find it
    let err = t(c2.lookup(super::ROOT_INODE, Path::new("doomed.txt")))
        .await
        .expect_err("expected NotFound from c2");

    assert_eq!(err.kind(), ErrorKind::NotFound);
}

#[tokio::test]
async fn test_multi_client_rename_visible() {
    let (mut c1, mut c2, dir) = setup_multi().await;

    std::fs::write(dir.path().join("orig.txt"), b"data").unwrap();

    let ino = t(c1.lookup(super::ROOT_INODE, Path::new("orig.txt")))
        .await
        .unwrap();

    // Client 1 renames
    t(c1.rename(ino, super::ROOT_INODE, Path::new("moved.txt")))
        .await
        .expect("rename failed");

    // Client 2 can find the new name
    let ino2 = t(c2.lookup(super::ROOT_INODE, Path::new("moved.txt")))
        .await
        .expect("c2 lookup new name failed");
    assert!(ino2 > 0);

    // Old name is gone
    let err = t(c2.lookup(super::ROOT_INODE, Path::new("orig.txt")))
        .await
        .expect_err("expected NotFound for old name");
    assert_eq!(err.kind(), ErrorKind::NotFound);

    // Content preserved
    let data = t(c2.read(ino2, 0, 100)).await.expect("read failed");
    assert_eq!(&data, b"data");
}

#[tokio::test]
async fn test_multi_client_independent_sessions() {
    // Two clients can each do full CRUD independently without interfering
    let (mut c1, mut c2, _dir) = setup_multi().await;

    let ino1 = t(c1.create_file(
        super::ROOT_INODE, 0o644, 0, Path::new("c1_only.txt"),
    ))
    .await
    .unwrap();

    let ino2 = t(c2.create_file(
        super::ROOT_INODE, 0o644, 0, Path::new("c2_only.txt"),
    ))
    .await
    .unwrap();

    // Each client writes to its own file
    t(c1.write(ino1, 0, b"client1 data")).await.unwrap();
    t(c2.write(ino2, 0, b"client2 data")).await.unwrap();

    // Each reads back its own file correctly
    let d1 = t(c1.read(ino1, 0, 100)).await.unwrap();
    let d2 = t(c2.read(ino2, 0, 100)).await.unwrap();
    assert_eq!(&d1, b"client1 data");
    assert_eq!(&d2, b"client2 data");

    // Cleanup: each removes its own file
    t(c1.remove(ino1)).await.unwrap();
    t(c2.remove(ino2)).await.unwrap();
}
