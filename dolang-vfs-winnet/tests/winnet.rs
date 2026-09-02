#![deny(warnings)]

#[cfg(not(windows))]
mod stub {
    use dolang_vfs::{Vfs, error::ErrorKind, server::Server};
    use dolang_vfs_winnet::{Group, User, account_policy};
    use tempfile::tempdir;

    #[tokio::test]
    async fn direct_dispatch_reports_unsupported() {
        let vfs = Vfs::direct().unwrap();
        let error = User::by_name(&vfs, "nobody").await.err().unwrap();
        assert_eq!(error.kind(), ErrorKind::Unsupported);
        let error = Group::by_name(&vfs, "nobody").await.err().unwrap();
        assert_eq!(error.kind(), ErrorKind::Unsupported);
        let error = account_policy(&vfs).await.err().unwrap();
        assert_eq!(error.kind(), ErrorKind::Unsupported);
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
        let error = User::by_name(&vfs, "nobody").await.err().unwrap();
        assert_eq!(error.kind(), ErrorKind::Unsupported);
        let error = Group::by_name(&vfs, "nobody").await.err().unwrap();
        assert_eq!(error.kind(), ErrorKind::Unsupported);
        let error = account_policy(&vfs).await.err().unwrap();
        assert_eq!(error.kind(), ErrorKind::Unsupported);
    }
}
