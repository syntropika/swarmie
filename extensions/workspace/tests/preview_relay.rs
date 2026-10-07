use base64::{Engine, engine::general_purpose::STANDARD};
use mpai::{Runtime, RuntimeExtension};
use mpai_workspace::WorkspaceExtension;
use serde_json::{Value, json};
use tokio::net::TcpStream;

async fn setup(extension: &WorkspaceExtension, project: &std::path::Path) -> Value {
    let opened = extension
        .invoke("project.open", json!({"path":project}))
        .await
        .unwrap();
    extension
        .invoke("preview.start", json!({"projectId":opened["id"]}))
        .await
        .unwrap()
}
fn body(response: &Value) -> String {
    String::from_utf8(STANDARD.decode(response["data"].as_str().unwrap()).unwrap()).unwrap()
}
#[tokio::test]
async fn registered_relay_rewrites_static_bundle_paths_without_changing_relative_urls_or_regexes() {
    let project = tempfile::tempdir().unwrap();
    let storage = tempfile::tempdir().unwrap();
    std::fs::create_dir(project.path().join("assets")).unwrap();
    std::fs::create_dir(project.path().join("docs")).unwrap();
    std::fs::write(project.path().join("index.html"),r#"<script type="module" src="/assets/app.js"></script><link href=/assets/style.css rel=stylesheet><img src="./logo.svg"><img src="//example.invalid/image"><base href="/"><script>const quote=/"/g; fetch('/data.json');</script>"#).unwrap();
    std::fs::write(project.path().join("assets/app.js"),r#"const expression=/test/; const quote=/"/g; if(ready) /'/g.test(value); const x=8/2; import('/assets/chunk.js'); fetch('/data.json'); const relative='./near.js';"#).unwrap();
    std::fs::write(
        project.path().join("assets/style.css"),
        r#"body{background:url(/assets/logo.svg)}@import '/assets/extra.css';"#,
    )
    .unwrap();
    std::fs::write(project.path().join("docs/index.html"), "<h1>Nested</h1>").unwrap();
    let extension = WorkspaceExtension::new().with_storage_root(storage.path());
    let started = setup(&extension, project.path()).await;
    let id = started["previewId"].as_str().unwrap();
    let prefix = started["relayPath"].as_str().unwrap();
    assert_eq!(prefix, format!("/preview/{id}/"));
    let response = extension
        .invoke("preview.read", json!({"previewId":id,"path":""}))
        .await
        .unwrap();
    assert_eq!(response["status"], 200);
    let html = body(&response);
    assert!(html.contains(&format!("src=\"{prefix}assets/app.js\"")));
    assert!(html.contains(&format!("href={prefix}assets/style.css")));
    assert!(html.contains("./logo.svg"));
    assert!(html.contains("//example.invalid/image"));
    assert!(html.contains("const quote=/\"/g;"));
    assert!(html.contains(&format!("fetch('{prefix}data.json')")));
    let js = body(
        &extension
            .invoke(
                "preview.read",
                json!({"previewId":id,"path":"assets/app.js"}),
            )
            .await
            .unwrap(),
    );
    assert!(js.contains("expression=/test/"));
    assert!(js.contains("quote=/\"/g"));
    assert!(js.contains("if(ready) /'/g.test(value)"));
    assert!(js.contains("8/2"));
    assert!(js.contains("'./near.js'"));
    assert!(js.contains(&format!("import('{prefix}assets/chunk.js')")));
    assert!(js.contains(&format!("fetch('{prefix}data.json')")));
    let css = body(
        &extension
            .invoke(
                "preview.read",
                json!({"previewId":id,"path":"assets/style.css"}),
            )
            .await
            .unwrap(),
    );
    assert!(css.contains(&format!("url({prefix}assets/logo.svg)")));
    let redirect = extension
        .invoke("preview.read", json!({"previewId":id,"path":"docs"}))
        .await
        .unwrap();
    assert_eq!(redirect["status"], 308);
    assert_eq!(redirect["location"], format!("{prefix}docs/"));
    assert!(
        body(
            &extension
                .invoke("preview.read", json!({"previewId":id,"path":"docs/"}))
                .await
                .unwrap()
        )
        .contains("Nested")
    );
    extension.shutdown().await;
    assert!(
        extension
            .invoke("preview.read", json!({"previewId":id}))
            .await
            .is_err()
    );
}
#[tokio::test]
async fn relay_rejects_unknown_ids_private_traversal_symlinks_and_oversized_files() {
    let project = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(project.path().join("index.html"), "<h1>Public</h1>").unwrap();
    std::fs::write(project.path().join(".private"), "private").unwrap();
    std::fs::create_dir(project.path().join(".git")).unwrap();
    std::fs::write(
        project.path().join("expansion.js"),
        "\"/a\",".repeat(500_000),
    )
    .unwrap();
    std::fs::write(outside.path().join("outside.txt"), "outside").unwrap();
    std::fs::File::create(project.path().join("large.bin"))
        .unwrap()
        .set_len(8 * 1024 * 1024 + 1)
        .unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(
        outside.path().join("outside.txt"),
        project.path().join("link.txt"),
    )
    .unwrap();
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(
            project.path().join(".private"),
            project.path().join("alias.txt"),
        )
        .unwrap();
        std::os::unix::fs::symlink(
            project.path().join(".git"),
            project.path().join("public-alias"),
        )
        .unwrap();
    }
    let extension = WorkspaceExtension::new().with_storage_root(outside.path().join("state"));
    let started = setup(&extension, project.path()).await;
    let id = started["previewId"].as_str().unwrap();
    assert!(
        extension
            .invoke(
                "preview.read",
                json!({"previewId":uuid::Uuid::new_v4().to_string()})
            )
            .await
            .is_err()
    );
    for path in [
        ".private",
        "%2eprivate",
        "%2e%2e/outside.txt",
        "a/../index.html",
        "a%5cb",
        "%00",
    ] {
        assert_eq!(
            extension
                .invoke("preview.read", json!({"previewId":id,"path":path}))
                .await
                .unwrap()["status"],
            403,
            "{path}"
        );
    }
    assert_eq!(
        extension
            .invoke(
                "preview.read",
                json!({"previewId":id,"path":"a".repeat(4097)})
            )
            .await
            .unwrap()["status"],
        414
    );
    assert_eq!(
        extension
            .invoke("preview.read", json!({"previewId":id,"path":"large.bin"}))
            .await
            .unwrap()["status"],
        413
    );
    assert_eq!(
        extension
            .invoke(
                "preview.read",
                json!({"previewId":id,"path":"expansion.js"})
            )
            .await
            .unwrap()["status"],
        413
    );
    #[cfg(unix)]
    assert_eq!(
        extension
            .invoke("preview.read", json!({"previewId":id,"path":"link.txt"}))
            .await
            .unwrap()["status"],
        404
    );
    assert!(
        extension
            .invoke(
                "preview.start",
                json!({"projectId":project.path(),"path":".git"})
            )
            .await
            .is_err()
    );
    #[cfg(unix)]
    {
        assert_eq!(
            extension
                .invoke("preview.read", json!({"previewId":id,"path":"alias.txt"}))
                .await
                .unwrap()["status"],
            403
        );
        assert!(
            extension
                .invoke(
                    "preview.start",
                    json!({"projectId":project.path(),"path":"public-alias"})
                )
                .await
                .is_err()
        );
    }
    extension
        .invoke("preview.stop", json!({"previewId":id}))
        .await
        .unwrap();
    assert!(
        extension
            .invoke("preview.read", json!({"previewId":id}))
            .await
            .is_err()
    );
}
#[tokio::test]
async fn runtime_shutdown_closes_preview_listener_and_invalidates_the_capability() {
    let project = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    std::fs::write(project.path().join("index.html"), "<h1>Preview</h1>").unwrap();
    let runtime = Runtime::builder()
        .extension(WorkspaceExtension::new().with_storage_root(state.path()))
        .build();
    let project = runtime
        .invoke_extension("workspace", "project.open", json!({"path":project.path()}))
        .await
        .unwrap();
    let preview = runtime
        .invoke_extension(
            "workspace",
            "preview.start",
            json!({"projectId":project["id"]}),
        )
        .await
        .unwrap();
    let address = preview["url"]
        .as_str()
        .unwrap()
        .strip_prefix("http://")
        .unwrap();
    assert!(TcpStream::connect(address).await.is_ok());
    runtime.shutdown().await.unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        while TcpStream::connect(address).await.is_ok() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert!(
        runtime
            .invoke_extension(
                "workspace",
                "preview.read",
                json!({"previewId":preview["previewId"]})
            )
            .await
            .is_err()
    );
    assert!(
        runtime
            .invoke_extension(
                "workspace",
                "preview.start",
                json!({"projectId":project["id"]})
            )
            .await
            .is_err()
    );
}
