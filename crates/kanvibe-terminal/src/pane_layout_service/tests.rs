use kanvibe_db::{Database, DatabaseLocation};
use kanvibe_domain::pane_layout::{PaneCommand, PaneLayoutType};

use super::{
    SavePaneLayoutInput, delete_pane_layout, get_all_pane_layouts, get_effective_pane_layout,
    get_project_pane_layout, save_pane_layout,
};

fn open_database(directory: &tempfile::TempDir) -> Database {
    let location = DatabaseLocation {
        database_path: directory.path().join("kanvibe.db"),
        seed_database_path: None,
    };
    Database::open(&location).unwrap()
}

fn pane(command: &str) -> PaneCommand {
    PaneCommand {
        position: 0,
        command: command.to_string(),
    }
}

fn global_input(layout_type: PaneLayoutType, command: &str) -> SavePaneLayoutInput {
    SavePaneLayoutInput {
        layout_type,
        panes: vec![pane(command)],
        project_id: None,
        is_global: true,
    }
}

#[test]
fn saving_global_layout_twice_updates_the_same_row() {
    let directory = tempfile::tempdir().unwrap();
    let database = open_database(&directory);

    let created = save_pane_layout(&database, global_input(PaneLayoutType::Single, "ls")).unwrap();
    let updated =
        save_pane_layout(&database, global_input(PaneLayoutType::Quad, "git status")).unwrap();

    assert_eq!(updated.id, created.id);
    assert_eq!(updated.layout_type, PaneLayoutType::Quad);
    assert_eq!(updated.panes, vec![pane("git status")]);
    assert_eq!(get_all_pane_layouts(&database).unwrap(), vec![updated]);
}

#[test]
fn effective_layout_falls_back_to_global_without_project_setting() {
    let directory = tempfile::tempdir().unwrap();
    let database = open_database(&directory);
    let global = save_pane_layout(&database, global_input(PaneLayoutType::Vertical2, "")).unwrap();

    let effective = get_effective_pane_layout(&database, Some("missing-project")).unwrap();

    assert_eq!(
        get_project_pane_layout(&database, "missing-project").unwrap(),
        None
    );
    assert_eq!(effective, Some(global.clone()));
    assert_eq!(
        get_effective_pane_layout(&database, None).unwrap(),
        Some(global)
    );
}

#[test]
fn deleting_reports_whether_a_row_was_removed() {
    let directory = tempfile::tempdir().unwrap();
    let database = open_database(&directory);
    let created = save_pane_layout(&database, global_input(PaneLayoutType::Single, "")).unwrap();

    assert!(delete_pane_layout(&database, &created.id).unwrap());
    assert!(!delete_pane_layout(&database, &created.id).unwrap());
    assert_eq!(get_all_pane_layouts(&database).unwrap(), vec![]);
}
