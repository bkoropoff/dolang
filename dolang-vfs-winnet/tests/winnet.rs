#![deny(warnings)]

#[cfg(not(windows))]
mod stub {
    use dolang_vfs::{Vfs, error::ErrorKind, server::Server};
    use dolang_vfs_winnet::{group, policy, share, user};
    use tempfile::tempdir;

    /// Every entry point reports the extension as unsupported off Windows.
    async fn assert_unsupported(vfs: &Vfs) {
        assert_eq!(
            user::by_name(vfs, "nobody").await.err().unwrap().kind(),
            ErrorKind::Unsupported
        );
        assert_eq!(
            group::by_name(vfs, "nobody").await.err().unwrap().kind(),
            ErrorKind::Unsupported
        );
        assert_eq!(
            policy::get(vfs).await.err().unwrap().kind(),
            ErrorKind::Unsupported
        );
        assert_eq!(
            share::by_name(vfs, "nobody").await.err().unwrap().kind(),
            ErrorKind::Unsupported
        );
        assert_eq!(
            share::enumerate(vfs)
                .next_entry()
                .await
                .err()
                .unwrap()
                .kind(),
            ErrorKind::Unsupported
        );
    }

    #[tokio::test]
    async fn direct_dispatch_reports_unsupported() {
        let vfs = Vfs::direct().unwrap();
        assert_unsupported(&vfs).await;
    }

    #[tokio::test]
    async fn remote_dispatch_reports_unsupported() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("vfs.sock");
        let server = Server::bind(&path).await.unwrap();
        tokio::spawn(async move {
            let _ = server.accept().await;
        });
        let vfs = Vfs::connect(&path).await.unwrap();
        assert_unsupported(&vfs).await;
    }
}
