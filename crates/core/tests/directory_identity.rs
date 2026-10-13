//! Directory bindings follow the retained filesystem object, not only its path text.
#[tokio::test]
async fn retained_directory_identity_survives_rename_and_rejects_replacement() {
    let temp = tempfile::tempdir().unwrap();
    let original = temp.path().join("working folder é");
    std::fs::create_dir(&original).unwrap();
    let (_, before) = swarmie::observe_directory(&original, false).await.unwrap();
    assert!(before.is_some());
    let moved = temp.path().join("moved folder");
    std::fs::rename(&original, &moved).unwrap();
    assert_eq!(
        swarmie::observe_directory(&moved, false).await.unwrap().1,
        before
    );
    std::fs::create_dir(&original).unwrap();
    assert_ne!(
        swarmie::observe_directory(&original, false)
            .await
            .unwrap()
            .1,
        before
    );
    let missing = temp.path().join("missing");
    assert!(
        swarmie::observe_directory(&missing, true)
            .await
            .unwrap()
            .1
            .is_none()
    );
    assert!(!missing.exists());
}
