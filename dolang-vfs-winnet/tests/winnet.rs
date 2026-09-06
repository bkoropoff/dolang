#![deny(warnings)]

#[cfg(not(windows))]
mod stub {
    use dolang_vfs::{Vfs, error::ErrorKind, path, server::Server};
    use dolang_vfs_winnet::{domain, group, machine, policy, share, user};
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
        assert_eq!(
            domain::status(vfs).await.err().unwrap().kind(),
            ErrorKind::Unsupported
        );
        assert_eq!(
            machine::info(vfs).await.err().unwrap().kind(),
            ErrorKind::Unsupported
        );
        assert_eq!(
            domain::join(vfs, domain::Join::new("corp.example.com".into()))
                .await
                .err()
                .unwrap()
                .kind(),
            ErrorKind::Unsupported
        );
        assert_eq!(
            domain::unjoin(vfs, domain::Unjoin::default())
                .await
                .err()
                .unwrap()
                .kind(),
            ErrorKind::Unsupported
        );
        assert_eq!(
            domain::rename(vfs, domain::Rename::new("NEWNAME".into()))
                .await
                .err()
                .unwrap()
                .kind(),
            ErrorKind::Unsupported
        );
        assert_eq!(
            domain::provision(
                vfs,
                domain::Provision::new("corp.example.com".into(), "WS01".into())
            )
            .await
            .err()
            .unwrap()
            .kind(),
            ErrorKind::Unsupported
        );
        assert_eq!(
            domain::apply_offline(
                vfs,
                domain::OfflineJoin::new(
                    vec![0, 1, 2, 3],
                    path::PathBuf::from_windows(r"C:\Windows")
                )
            )
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
