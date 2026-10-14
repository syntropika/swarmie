use std::{fs, path::Path};
use swarmie_skills::{Catalog, Root, canonical_roots};

fn skill(root: &Path, folder: &str, name: &str) -> std::path::PathBuf {
    let directory = root.join(folder);
    fs::create_dir_all(directory.join("references")).unwrap();
    fs::write(directory.join("SKILL.md"), format!("---\nname: {name}\ndescription: |\n  Read the complete\n  supporting folder.\nuser-invocable: no\ndisable-model-invocation: true\n---\nRead references/detail.txt.\n")).unwrap();
    fs::write(directory.join("references/detail.txt"), "reference").unwrap();
    directory
}

#[test]
fn scope_conflicts_do_not_merge_different_origins_or_walk_supporting_skills() {
    let root = tempfile::tempdir().unwrap();
    let project = root.path().join("project");
    let user = root.path().join("user");
    let first = skill(&project, "first", "same-name");
    skill(&first.join("references"), "nested", "must-not-discover");
    skill(&user, "second", "same-name");
    let catalog = Catalog::discover(&[
        Root {
            path: project,
            scope: "project",
        },
        Root {
            path: user,
            scope: "user",
        },
    ]);
    assert_eq!(catalog.skills.len(), 2);
    assert_eq!(catalog.skills[0].scope, "project");
    assert_eq!(catalog.skills[1].scope, "user");
    assert_eq!(
        catalog.skills[0].conflicts,
        [catalog.skills[1].path.clone()]
    );
    assert!(!catalog.skills[0].user_invocable);
    assert!(!catalog.skills[0].model_invocable);
    assert!(catalog.skills[0].usable());
    assert!(
        catalog.skills[0]
            .description
            .as_ref()
            .unwrap()
            .contains("supporting folder")
    );
    assert_ne!(catalog.skills[0].alias(), catalog.skills[1].alias());
    assert!(catalog.skills[0].unchanged());
    fs::write(&catalog.skills[0].path, "changed").unwrap();
    assert!(!catalog.skills[0].unchanged());
}

#[cfg(unix)]
#[test]
fn directory_aliases_deduplicate_with_origins_and_cycles_or_broken_links_are_bounded() {
    use std::os::unix::fs::symlink;
    let root = tempfile::tempdir().unwrap();
    let shared = root.path().join("shared");
    let directory = skill(&shared, "one", "one");
    symlink(&directory, shared.join("alias")).unwrap();
    symlink(&shared, shared.join("cycle")).unwrap();
    symlink(root.path().join("missing"), shared.join("broken")).unwrap();
    let alias_root = root.path().join("root-alias");
    symlink(&shared, &alias_root).unwrap();
    let catalog = Catalog::discover(&[
        Root {
            path: shared,
            scope: "project",
        },
        Root {
            path: alias_root,
            scope: "user",
        },
    ]);
    assert_eq!(catalog.skills.len(), 1);
    assert!(
        catalog.skills[0]
            .origins
            .iter()
            .any(|origin| origin.scope == "user")
    );
    assert!(
        catalog.skills[0]
            .origins
            .iter()
            .any(|origin| origin.path.ends_with("alias"))
    );
    assert!(!catalog.truncated);
    assert!(
        catalog
            .diagnostics
            .iter()
            .any(|error| error.contains("broken"))
    );
    assert_eq!(
        catalog.skills[0].path,
        directory.join("SKILL.md").canonicalize().unwrap()
    );
}

#[test]
fn metadata_failures_are_visible_and_roots_stop_at_the_repository_boundary() {
    let root = tempfile::tempdir().unwrap();
    let project = root.path().join("repo");
    let cwd = project.join("packages/app");
    fs::create_dir_all(&cwd).unwrap();
    fs::create_dir(project.join(".git")).unwrap();
    let roots = canonical_roots(&cwd, Some(root.path()));
    assert_eq!(roots.len(), 4);
    assert_eq!(roots[2].path, project.join(".agents/skills"));
    let invalid = roots[0].path.join("invalid");
    fs::create_dir_all(&invalid).unwrap();
    fs::write(
        invalid.join("SKILL.md"),
        "---\nname: [unclosed\n---\nInstructions",
    )
    .unwrap();
    let catalog = Catalog::discover(&roots);
    assert_eq!(catalog.skills.len(), 1);
    assert!(!catalog.skills[0].usable());
    assert!(
        catalog.skills[0]
            .diagnostics
            .iter()
            .any(|error| error.contains("Invalid skill YAML"))
    );
}

#[cfg(unix)]
#[test]
fn generated_overlays_are_excluded_even_when_linked_into_a_discovery_root() {
    let root = tempfile::tempdir().unwrap();
    let shared = root.path().join("shared");
    let target = skill(&shared, "check", "check");
    let overlay = root.path().join("overlay");
    fs::create_dir_all(overlay.join(".claude/skills")).unwrap();
    fs::write(
        overlay.join(".swarmie-skill-overlay.json"),
        serde_json::json!({"format":"swarmie-skill-overlay-v1","root":overlay}).to_string(),
    )
    .unwrap();
    std::os::unix::fs::symlink(&target, overlay.join(".claude/skills/alias")).unwrap();
    let discovery = root.path().join("discovery");
    fs::create_dir_all(&discovery).unwrap();
    std::os::unix::fs::symlink(&overlay, discovery.join("generated")).unwrap();
    let catalog = Catalog::discover(&[
        Root {
            path: discovery,
            scope: "project",
        },
        Root {
            path: shared,
            scope: "user",
        },
    ]);
    assert_eq!(catalog.skills.len(), 1);
    assert_eq!(catalog.skills[0].origins.len(), 1);
    assert_eq!(catalog.skills[0].scope, "user");
}
