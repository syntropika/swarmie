use std::{fs, path::Path};
use swarmie_project_context::{Configuration, Error, Limits, ProjectContext, Scope};
use swarmie_providers::{AgentContext, Message, SourceKind};

fn write(path: impl AsRef<Path>, value: impl AsRef<[u8]>) {
    let path = path.as_ref();
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, value).unwrap();
}
fn skill(root: &Path, folder: &str, name: &str, body: &str) {
    write(
        root.join(format!(".agents/skills/{folder}/SKILL.md")),
        format!("---\nname: {name}\ndescription: Work on the fixture task.\n---\n{body}\n"),
    );
}
fn project(root: &Path, cwd: &Path) -> ProjectContext {
    ProjectContext::new(Scope::new(root, cwd).unwrap(), Configuration::default()).unwrap()
}

#[test]
fn distinct_origins_with_one_name_require_exact_selection_and_keep_supporting_files() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    skill(root, "first", "review", "First origin instructions");
    skill(root, "second", "review", "Second origin instructions");
    write(
        root.join(".agents/skills/first/references/detail.txt"),
        "Reference marker only",
    );
    write(
        root.join(".agents/skills/first/assets/picture.bin"),
        [0xff, 0x00],
    );
    let project = project(root, root);
    let inventory = project.inspect().unwrap();
    assert_eq!(inventory.skills.len(), 2);
    assert!(matches!(inventory.select("review"), Err(Error::Ambiguous)));
    let first = inventory
        .skills
        .iter()
        .find(|s| s.origin.ends_with("first"))
        .unwrap();
    let selected = inventory
        .select(&format!(
            "{}@{}",
            first.selection.id, first.selection.revision
        ))
        .unwrap();
    let captured = project
        .capture("node", "alice", std::slice::from_ref(&selected))
        .unwrap();
    let body = captured
        .parts()
        .iter()
        .find(|part| part.kind() == SourceKind::SkillInstructions)
        .unwrap();
    assert!(body.content().contains("First origin"));
    assert!(!body.content().contains("Second origin"));
    assert!(!body.content().contains("Reference marker"));
    assert_eq!(
        project.read(&selected, "references/detail.txt").unwrap(),
        "Reference marker only"
    );
    assert!(matches!(
        project.read(&selected, "assets/picture.bin"),
        Err(Error::Invalid(_))
    ));
    assert!(!root.join("unexpected-script-output").exists());
}

#[test]
fn a_supporting_file_change_invalidates_the_whole_selected_package() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    skill(root, "writer", "writer", "Read the reference.");
    let reference = root.join(".agents/skills/writer/reference.md");
    write(&reference, "revision one");
    let project = project(root, root);
    let selected = project.inspect().unwrap().select("writer").unwrap();
    write(&reference, "revision two");
    assert!(matches!(
        project.read(&selected, "reference.md"),
        Err(Error::Changed)
    ));
    assert!(matches!(
        project.capture("node", "alice", &[selected]),
        Err(Error::Changed)
    ));
}

#[test]
fn nested_project_instructions_are_ordered_and_never_include_parent_roots() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("project");
    let cwd = root.join("src/feature");
    fs::create_dir_all(&cwd).unwrap();
    write(dir.path().join("AGENTS.md"), "Outside scope");
    write(root.join("AGENTS.md"), "Project root");
    write(root.join("src/AGENTS.md"), "Source folder");
    write(cwd.join("AGENTS.md"), "Feature folder");
    let captured = project(&root, &cwd).capture("node", "alice", &[]).unwrap();
    let contents: Vec<_> = captured.parts().iter().map(|part| part.content()).collect();
    assert_eq!(
        contents,
        vec!["Project root", "Source folder", "Feature folder"]
    );
    let context = AgentContext {
        instructions: "Host instruction".into(),
        sources: Some(captured.at(2)),
        messages: vec![
            Message::Assistant(swarmie_providers::AssistantMessage {
                provider: None,
                content: "Historical checkpoint".into(),
                tool_calls: vec![],
            }),
            Message::User {
                content: "Retained older input".into(),
            },
            Message::User {
                content: "Current request".into(),
            },
        ],
    };
    let view = context.model_messages().unwrap();
    assert_eq!(view.len(), 4);
    assert_eq!(view.first(), context.messages.first());
    assert!(
        matches!(view.get(2),Some(Message::User{content}) if content.contains("Feature folder"))
    );
    assert_eq!(view.last(), context.messages.last());
    assert_eq!(context.messages.len(), 3);
}

#[test]
fn manual_skills_are_hidden_from_model_catalog_but_available_for_explicit_use() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(
        root.join(".agents/skills/manual/SKILL.md"),
        "---\nname: manual\ndescription: Work explicitly.\ndisable-model-invocation: true\n---\nManual body\n",
    );
    let project = project(root, root);
    let selected = project.inspect().unwrap().select("manual").unwrap();
    let without = project.capture("node", "alice", &[]).unwrap();
    assert!(without.parts().is_empty());
    let explicit = project.capture("node", "alice", &[selected]).unwrap();
    assert_eq!(explicit.parts().len(), 1);
    assert_eq!(explicit.parts()[0].kind(), SourceKind::SkillInstructions);
}

#[test]
fn malformed_or_executor_dependent_metadata_is_diagnostic_and_cannot_grant_tools() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(
        root.join(".agents/skills/broken/SKILL.md"),
        "---\nname: [broken\n---\nBody",
    );
    write(
        root.join(".agents/skills/permissions/SKILL.md"),
        "---\nname: permissions\ndescription: Inspect permissions.\nallowed-tools: shell\n---\nBody",
    );
    let project = project(root, root);
    let inventory = project.inspect().unwrap();
    assert_eq!(inventory.diagnostics.len(), 1);
    assert_eq!(inventory.skills.len(), 1);
    assert!(matches!(
        inventory.select("permissions"),
        Err(Error::Unsupported)
    ));
    assert!(
        project
            .capture("node", "alice", &[])
            .unwrap()
            .parts()
            .is_empty()
    );
}

#[test]
fn exact_frontmatter_boundaries_and_folded_descriptions_are_supported() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(
        root.join(".agents/skills/folded/SKILL.md"),
        "\u{feff}---\r\nname: folded\r\ndescription: >\r\n  A description\r\n  on two lines.\r\n---\r\nBody marker\r\n",
    );
    write(
        root.join(".agents/skills/bad/SKILL.md"),
        "---\nname: bad\ndescription: Example\n---suffix\nBody",
    );
    let inventory = project(root, root).inspect().unwrap();
    assert_eq!(inventory.skills.len(), 1);
    assert_eq!(inventory.diagnostics.len(), 1);
    assert!(inventory.skills[0].description.contains("on two lines"));
}

#[test]
fn paths_cannot_escape_or_reinterpret_a_package() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    skill(root, "reader", "reader", "Read instructions");
    let project = project(root, root);
    let selected = project.inspect().unwrap().select("reader").unwrap();
    for path in [
        "../AGENTS.md",
        "/etc/passwd",
        "references/../../AGENTS.md",
        "a\\b",
        "",
    ] {
        assert!(matches!(
            project.read(&selected, path),
            Err(Error::Invalid(_))
        ));
    }
}

#[test]
fn configured_limits_refuse_content_instead_of_returning_a_partial_catalog() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    skill(root, "one", "one", "Body one");
    skill(root, "two", "two", "Body two");
    let limits = Limits {
        max_skills: 1,
        ..Limits::default()
    };
    let owner = ProjectContext::new(
        Scope::new(root, root).unwrap(),
        Configuration {
            limits,
            ..Configuration::default()
        },
    )
    .unwrap();
    assert!(matches!(owner.inspect(), Err(Error::Limit)));
    write(root.join("AGENTS.md"), "long instruction");
    let limits = Limits {
        max_text_bytes: 4,
        ..Limits::default()
    };
    let owner = ProjectContext::new(
        Scope::new(root, root).unwrap(),
        Configuration {
            limits,
            ..Configuration::default()
        },
    )
    .unwrap();
    assert!(matches!(
        owner.capture("node", "alice", &[]),
        Err(Error::Limit)
    ));
}

#[cfg(unix)]
#[test]
fn linked_origins_deduplicate_but_escaping_and_cyclic_resources_are_refused() {
    use std::os::unix::fs::symlink;
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    skill(root, "reader", "reader", "Read instructions");
    let package = root.join(".agents/skills/reader");
    symlink(&package, root.join(".agents/skills/alias")).unwrap();
    let inventory = project(root, root).inspect().unwrap();
    assert_eq!(inventory.skills.len(), 1);
    assert_eq!(inventory.skills[0].aliases.len(), 2);
    write(root.join("outside.txt"), "Private outside package");
    symlink(root.join("outside.txt"), package.join("escape.txt")).unwrap();
    let invalid = project(root, root).inspect().unwrap();
    assert!(invalid.skills.is_empty());
    assert!(!invalid.diagnostics.is_empty());
    fs::remove_file(package.join("escape.txt")).unwrap();
    symlink(&package, package.join("cycle")).unwrap();
    assert!(project(root, root).inspect().unwrap().skills.is_empty());
}

#[cfg(unix)]
#[test]
fn a_project_instruction_link_must_stay_inside_its_authorized_root() {
    use std::os::unix::fs::symlink;
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("root");
    fs::create_dir_all(&root).unwrap();
    write(dir.path().join("private.md"), "Outside source");
    symlink(dir.path().join("private.md"), root.join("AGENTS.md")).unwrap();
    assert!(matches!(
        project(&root, &root).capture("node", "alice", &[]),
        Err(Error::Invalid(_))
    ));
}

#[test]
fn absent_configured_sources_and_changed_captures_fail_explicitly() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let configured = ProjectContext::new(
        Scope::new(root, root).unwrap(),
        Configuration {
            personal_instructions: vec![root.join("missing.md")],
            ..Default::default()
        },
    )
    .unwrap();
    assert!(matches!(
        configured.capture("node", "user", &[]),
        Err(Error::Io(_))
    ));
    write(root.join("AGENTS.md"), "Captured marker");
    let captured = project(root, root).capture("node", "user", &[]).unwrap();
    let context = AgentContext {
        instructions: String::new(),
        sources: Some(captured),
        messages: vec![Message::User {
            content: "Admitted".into(),
        }],
    };
    let mut serialized = serde_json::to_value(&context).unwrap();
    serialized["sources"]["parts"][0]["content"] =
        serde_json::json!("Changed without matching evidence");
    let changed: AgentContext = serde_json::from_value(serialized).unwrap();
    assert!(changed.model_messages().is_err());
    let mut lost = context;
    lost.messages.clear();
    assert!(lost.model_messages().is_err());
}

#[test]
fn one_inventory_owner_has_an_explicit_refresh_boundary_and_a_total_read_budget() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    skill(root, "one", "one", "Body one");
    let first = project(root, root);
    assert_eq!(first.inspect().unwrap().skills.len(), 1);
    skill(root, "two", "two", "Body two");
    assert_eq!(first.inspect().unwrap().skills.len(), 1);
    assert_eq!(project(root, root).inspect().unwrap().skills.len(), 2);
    let limits = Limits {
        max_package_bytes: 120,
        max_library_bytes: 120,
        max_text_bytes: 120,
        ..Default::default()
    };
    let limited = ProjectContext::new(
        Scope::new(root, root).unwrap(),
        Configuration {
            limits,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(matches!(limited.inspect(), Err(Error::Limit)));
}

#[cfg(unix)]
#[test]
fn dangling_instruction_links_and_ambiguous_package_names_are_not_silently_skipped() {
    use std::os::unix::fs::symlink;
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    symlink(root.join("missing.md"), root.join("AGENTS.md")).unwrap();
    assert!(matches!(
        project(root, root).capture("node", "user", &[]),
        Err(Error::Io(_))
    ));
    skill(root, "one", "one", "Body one");
    write(root.join(".agents/skills/one/a\\b"), "Ambiguous resource");
    let inventory = project(root, root).inspect().unwrap();
    assert!(inventory.skills.is_empty());
    assert_eq!(inventory.diagnostics.len(), 1);
}
