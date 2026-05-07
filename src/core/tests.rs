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

    let stats = t(handler.stats()).await.expect("stats failed");

    assert!(stats.block_size > 0);
    assert!(stats.total_blocks > 0);
    assert!(stats.total_inodes > 0);
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

    // TempDir is freshly created — no entries beyond . and ..
    let entries = t(handler.read_dir(super::ROOT_INODE))
        .await
        .expect("read_dir failed");

    // All names should be . or ..
    for e in &entries {
        let name = std::str::from_utf8(&e.filename).unwrap_or("");
        assert!(
            name == "." || name == "..",
            "unexpected entry: {name}"
        );
    }
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
