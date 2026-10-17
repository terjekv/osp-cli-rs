use super::*;
use chrono::{TimeZone, Utc};

fn history_config() -> HistoryConfig {
    HistoryConfig {
        enabled: true,
        max_entries: 10,
        dedupe: false,
        ..HistoryConfig::default()
    }
}

#[test]
fn wildcard_matching_handles_prefix_and_infix() {
    assert!(matches_pattern("ldap user *", "ldap user bob"));
    assert!(matches_pattern("*token*", "auth token read"));
    assert!(!matches_pattern("auth", "auth token"));
    assert!(matches_pattern("auth*", "auth token"));
    assert!(matches_pattern("*user", "ldap user"));
    assert!(!matches_pattern("*user", "ldap user bob"));
}

#[test]
fn excluded_commands_respect_prefixes_and_patterns() {
    let excludes = vec![
        "help".to_string(),
        "exit".to_string(),
        "quit".to_string(),
        "history list".to_string(),
    ];
    assert!(is_excluded_command("help", &excludes));
    assert!(is_excluded_command("history list", &excludes));
    assert!(!is_excluded_command("history prune 10", &[]));
    assert!(is_excluded_command("ldap user --help", &[]));
    assert!(is_excluded_command(
        "login oistes",
        &[String::from("login *")]
    ));
}

#[test]
fn list_entries_filters_shell_and_excludes() {
    let shell = HistoryShellContext::new("ldap");
    let config = HistoryConfig {
        exclude_patterns: ["user *"].into_iter().map(String::from).collect(),
        shell_context: shell,
        ..history_config()
    };
    let mut store = OspHistoryStore::new(config);
    let _ = store
        .save(HistoryItem::from_command_line("ldap user alice"))
        .expect("save should succeed");
    let _ = store
        .save(HistoryItem::from_command_line("ldap netgroup ucore"))
        .expect("save should succeed");
    let _ = store
        .save(HistoryItem::from_command_line("mreg host a"))
        .expect("save should succeed");

    let entries = store.list_entries_for(store.shell_prefix().as_deref());
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].command, "netgroup ucore");
    assert_eq!(entries[1].command, "mreg host a");
}

#[test]
fn list_entries_tracks_live_shell_context_updates() {
    let shell = HistoryShellContext::default();
    let config = HistoryConfig {
        shell_context: shell.clone(),
        ..history_config()
    };
    let mut store = OspHistoryStore::new(config);
    let _ = store
        .save(HistoryItem::from_command_line("ldap user alice"))
        .expect("save should succeed");
    let _ = store
        .save(HistoryItem::from_command_line("mreg host a"))
        .expect("save should succeed");

    shell.set_prefix("ldap");
    let ldap_entries = store.list_entries_for(store.shell_prefix().as_deref());
    assert_eq!(ldap_entries.len(), 1);
    assert_eq!(ldap_entries[0].command, "user alice");

    shell.set_prefix("mreg");
    let mreg_entries = store.list_entries_for(store.shell_prefix().as_deref());
    assert_eq!(mreg_entries.len(), 1);
    assert_eq!(mreg_entries[0].command, "host a");

    shell.clear();
    let root_entries = store.list_entries_for(store.shell_prefix().as_deref());
    assert_eq!(root_entries.len(), 2);
}

#[test]
fn explicit_scope_queries_override_live_shell_context() {
    let shell = HistoryShellContext::default();
    let config = HistoryConfig {
        shell_context: shell.clone(),
        ..history_config()
    };
    let mut store = OspHistoryStore::new(config);
    let _ = store
        .save(HistoryItem::from_command_line("ldap user alice"))
        .expect("save should succeed");
    let _ = store
        .save(HistoryItem::from_command_line("mreg host a"))
        .expect("save should succeed");

    shell.set_prefix("ldap");
    let mreg_entries = store.list_entries_for(Some("mreg"));
    assert_eq!(mreg_entries.len(), 1);
    assert_eq!(mreg_entries[0].command, "host a");

    let removed = store
        .prune_for(0, Some("mreg"))
        .expect("prune should succeed");
    assert_eq!(removed, 1);

    let root_entries = store.list_entries_for(None);
    assert_eq!(root_entries.len(), 1);
    assert_eq!(root_entries[0].command, "ldap user alice");
}

#[test]
fn save_expands_history_and_dedupes_with_shell_scope() {
    let shell = HistoryShellContext::new("ldap");
    let config = HistoryConfig {
        dedupe: true,
        shell_context: shell,
        ..history_config()
    };
    let mut store = OspHistoryStore::new(config);

    let first = store
        .save(HistoryItem::from_command_line("user alice"))
        .expect("save should succeed");
    assert_eq!(first.command_line, "ldap user alice");

    let duplicate = store
        .save(HistoryItem::from_command_line("!!"))
        .expect("history expansion should succeed");
    assert_eq!(duplicate.command_line, "!!");
    assert_eq!(
        store
            .list_entries_for(store.shell_prefix().as_deref())
            .len(),
        1
    );

    let second = store
        .save(HistoryItem::from_command_line("netgroup ops"))
        .expect("save should succeed");
    assert_eq!(second.command_line, "ldap netgroup ops");

    let recent = store.recent_commands_for(store.shell_prefix().as_deref());
    assert_eq!(recent, vec!["ldap user alice", "ldap netgroup ops"]);
    let visible = store.list_entries_for(store.shell_prefix().as_deref());
    assert_eq!(visible[0].command, "user alice");
    assert_eq!(visible[1].command, "netgroup ops");
}

#[test]
fn search_respects_filters_direction_bounds_and_skip_logic() {
    let config = HistoryConfig {
        shell_context: HistoryShellContext::default(),
        ..history_config()
    };
    let mut store = OspHistoryStore::new(config);

    let mut first = HistoryItem::from_command_line("ldap user alice");
    first.start_timestamp = Some(Utc.timestamp_millis_opt(1_000).single().unwrap());
    store.save(first).expect("save should succeed");

    let mut second = HistoryItem::from_command_line("ldap user bob");
    second.start_timestamp = Some(Utc.timestamp_millis_opt(2_000).single().unwrap());
    store.save(second).expect("save should succeed");

    let mut third = HistoryItem::from_command_line("mreg host a");
    third.start_timestamp = Some(Utc.timestamp_millis_opt(3_000).single().unwrap());
    store.save(third).expect("save should succeed");

    let mut filter = SearchFilter::anything(None);
    filter.command_line = Some(CommandLineSearch::Prefix("ldap".to_string()));

    let forward = SearchQuery {
        direction: SearchDirection::Forward,
        start_time: Some(Utc.timestamp_millis_opt(500).single().unwrap()),
        end_time: Some(Utc.timestamp_millis_opt(1_500).single().unwrap()),
        start_id: None,
        end_id: Some(HistoryItemId::new(2)),
        limit: Some(5),
        filter,
    };
    let results = store.search(forward).expect("search should succeed");
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].command_line, "ldap user alice");

    let mut backward = SearchQuery::everything(SearchDirection::Backward, None);
    backward.start_id = Some(HistoryItemId::new(1));
    backward.limit = Some(2);
    let results = store.search(backward).expect("search should succeed");
    let commands = results
        .iter()
        .map(|item| item.command_line.as_str())
        .collect::<Vec<_>>();
    assert_eq!(commands, vec!["ldap user alice"]);
    assert_eq!(
        store
            .count(SearchQuery::everything(SearchDirection::Forward, None))
            .expect("count should succeed"),
        3
    );
}

#[cfg_attr(miri, ignore = "history store filesystem integration test")]
#[test]
fn persisted_records_skip_invalid_lines_and_trim_to_capacity() {
    let temp_dir = make_temp_dir("osp-repl-history-load");
    let path = temp_dir.join("history.jsonl");
    std::fs::write(
        &path,
        concat!(
            "\n",
            "{\"id\":5,\"command_line\":\"first\",\"timestamp_ms\":10}\n",
            "not-json\n",
            "{\"id\":6,\"command_line\":\"   \",\"timestamp_ms\":20}\n",
            "{\"id\":7,\"command_line\":\"second\",\"timestamp_ms\":30}\n"
        ),
    )
    .expect("history fixture should be written");

    let store = OspHistoryStore::new(HistoryConfig {
        path: Some(path),
        max_entries: 1,
        shell_context: HistoryShellContext::default(),
        ..history_config()
    });

    let entries = store.list_entries_for(None);
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].id, 1);
    assert_eq!(entries[0].command, "second");
}

#[cfg_attr(miri, ignore = "history store filesystem integration test")]
#[test]
fn shared_history_supports_save_load_prune_clear_and_sync() {
    let temp_dir = make_temp_dir("osp-repl-shared-history");
    let path = temp_dir.join("history.jsonl");
    let mut history = SharedHistory::new(HistoryConfig {
        path: Some(path.clone()),
        max_entries: 8,
        shell_context: HistoryShellContext::default(),
        ..history_config()
    });

    history
        .with(|store| {
            store.stage_command_line("ldap user")?;
            store.finalize_execution("ldap user", "ldap user alice", true)
        })
        .expect("history lock should be available")
        .expect("save should succeed");
    history
        .with(|store| store.save_command_line("config get ui.format"))
        .expect("history lock should be available")
        .expect("save should succeed");
    assert!(
        history
            .with(|store| store.history_enabled())
            .unwrap_or(false)
    );
    assert_eq!(
        history
            .with(|store| store.recent_commands_for(store.shell_prefix().as_deref()))
            .unwrap_or_default()
            .len(),
        2
    );
    assert_eq!(
        history
            .load(HistoryItemId::new(0))
            .expect("load should succeed")
            .command_line,
        "ldap user alice"
    );

    assert_eq!(
        history
            .with(|store| store.prune_for(1, store.shell_prefix().as_deref()))
            .expect("history lock should be available")
            .expect("prune should succeed"),
        1
    );
    assert_eq!(
        history
            .with(|store| store.list_entries_for(store.shell_prefix().as_deref()))
            .unwrap_or_default()
            .len(),
        1
    );
    history.sync().expect("sync should succeed");
    assert!(path.exists());
    assert_eq!(
        history
            .with(|store| store
                .clear_matching_for("*", None, false, true)
                .map(|entries| entries.len()))
            .expect("history lock should be available")
            .expect("clear should succeed"),
        1
    );
    assert!(
        history
            .with(|store| store.list_entries_for(store.shell_prefix().as_deref()))
            .unwrap_or_default()
            .is_empty()
    );
    History::clear(&mut history).expect("clear should succeed");
    assert!(!path.exists());
}

#[test]
fn shell_prefix_helpers_normalize_and_round_trip_commands() {
    assert_eq!(
        normalize_shell_prefix(" ldap ".to_string()),
        Some("ldap ".to_string())
    );
    assert_eq!(
        normalize_scope_prefix(Some("ldap")),
        Some("ldap ".to_string())
    );
    assert!(command_matches_shell_prefix(
        "ldap user alice",
        Some("ldap ")
    ));
    assert_eq!(
        apply_shell_prefix("user alice", Some("ldap ")),
        "ldap user alice"
    );
    assert_eq!(
        apply_shell_prefix("ldap user alice", Some("ldap ")),
        "ldap user alice"
    );
    assert_eq!(
        strip_shell_prefix("ldap user alice", Some("ldap ")),
        "user alice"
    );
}

#[test]
fn unsupported_history_mutations_surface_feature_errors() {
    let mut store = OspHistoryStore::new(HistoryConfig {
        max_entries: 4,
        shell_context: HistoryShellContext::default(),
        ..history_config()
    });

    let update_err = store
        .update(HistoryItemId::new(0), &|item| item)
        .expect_err("update should stay unsupported");
    let delete_err = store
        .delete(HistoryItemId::new(0))
        .expect_err("delete should stay unsupported");

    assert!(update_err.to_string().contains("updating entries"));
    assert!(delete_err.to_string().contains("removing entries"));
    assert_eq!(store.session(), None);
}

#[test]
fn load_missing_history_item_returns_not_found_error() {
    let store = OspHistoryStore::new(HistoryConfig {
        max_entries: 4,
        shell_context: HistoryShellContext::default(),
        ..history_config()
    });

    let err = store
        .load(HistoryItemId::new(7))
        .expect_err("missing entry should fail");
    assert!(err.to_string().contains("history item not found"));
}

#[test]
fn disabled_history_returns_original_item_without_persisting_records() {
    let mut store = OspHistoryStore::new(HistoryConfig {
        enabled: false,
        dedupe: true,
        shell_context: HistoryShellContext::default(),
        ..history_config()
    });

    let item = store
        .save(HistoryItem::from_command_line("ldap user alice"))
        .expect("disabled history should be a no-op");

    assert_eq!(item.command_line, "ldap user alice");
    assert!(
        store
            .list_entries_for(store.shell_prefix().as_deref())
            .is_empty()
    );
    assert!(
        store
            .recent_commands_for(store.shell_prefix().as_deref())
            .is_empty()
    );
}

#[test]
fn history_never_records_recalls_help_or_inline_config_secrets() {
    let mut history = SharedHistory::new(history_config());
    assert!(matches_pattern("*", "anything"));
    assert!(!matches_pattern("  ", "anything"));
    assert!(!matches_pattern("a*b*c", "a c"));

    for command in [
        "   ",
        "sudo !!",
        "config set auth.token hunter2",
        "config set --secrets extensions.uio.user alice",
        "config set 'unterminated",
        "config set --no-such-flag ui.format json",
        "config set",
    ] {
        assert!(is_excluded_command(command, &[]), "{command}");
    }
    for command in [
        "sudox !!",
        "config get auth.token",
        "config set ui.format json",
    ] {
        assert!(!is_excluded_command(command, &[]), "{command}");
    }

    for command in [
        "config set auth.password hunter2",
        "config set ui.format json",
    ] {
        History::save(&mut history, HistoryItem::from_command_line(command))
            .expect("save should succeed");
    }
    assert_eq!(
        history
            .with(|store| store.recent_commands_for(store.shell_prefix().as_deref()))
            .unwrap_or_default(),
        vec!["config set ui.format json"]
    );
}

#[cfg_attr(miri, ignore = "history store filesystem integration test")]
#[test]
fn file_scoped_history_loads_legacy_records_and_drops_rejected_commands() {
    let temp_dir = make_temp_dir("osp-repl-history-profiles");
    let path = temp_dir.join("history.jsonl");
    std::fs::write(
        &path,
        concat!(
            "{\"id\":0,\"command_line\":\"ldap user legacy\"}\n",
            "{\"id\":1,\"command_line\":\"ldap user ops\",\"profile\":\"ops\"}\n",
            "{\"id\":2,\"command_line\":\"ldap user alice\",\"profile\":\"dev\",\"cwd\":\"/srv\",\"hostname\":\"ops-a\",\"exit_status\":0}\n",
        ),
    )
    .expect("history fixture should be written");
    let shell = HistoryShellContext::new("ldap");
    let mut history = SharedHistory::new(HistoryConfig {
        path: Some(path.clone()),

        shell_context: shell.clone(),
        ..history_config()
    });
    assert_eq!(
        history
            .with(|store| store.recent_commands_for(store.shell_prefix().as_deref()))
            .unwrap_or_default(),
        vec!["ldap user legacy", "ldap user ops", "ldap user alice"]
    );

    history
        .with(|store| store.save_command_line("user bob"))
        .expect("history lock should be available")
        .expect("save should succeed");
    History::save(&mut history, HistoryItem::from_command_line("user typo"))
        .expect("save should succeed");
    history
        .with(|store| store.finalize_execution("user other", "user other", false))
        .expect("history lock should be available")
        .expect("an unrelated finalization leaves the pending record alone");
    history
        .with(|store| store.finalize_execution("user typo", "user typo-expanded", false))
        .expect("history lock should be available")
        .expect("a rejected command should be dropped");
    let visible = history
        .with(|store| store.list_entries_for(store.shell_prefix().as_deref()))
        .unwrap_or_default()
        .into_iter()
        .map(|entry| entry.command)
        .collect::<Vec<_>>();
    assert_eq!(
        visible,
        vec!["user legacy", "user ops", "user alice", "user bob"]
    );

    let count = |filter: SearchFilter| {
        let mut query = SearchQuery::everything(SearchDirection::Forward, None);
        query.filter = filter;
        History::count(&history, query).expect("count should succeed")
    };
    let mut exact = SearchFilter::anything(None);
    exact.command_line = Some(CommandLineSearch::Exact("user bob".to_string()));
    assert_eq!(count(exact), 1);
    let mut substring = SearchFilter::anything(None);
    substring.command_line = Some(CommandLineSearch::Substring("ali".to_string()));
    assert_eq!(count(substring), 1);

    assert!(History::update(&mut history, HistoryItemId::new(0), &|item| item).is_err());
    assert!(History::delete(&mut history, HistoryItemId::new(0)).is_err());
    assert_eq!(History::session(&history), None);

    let _ = history.with(|store| store.remember_command("   "));
    let _ = history.with(|store| store.remember_command("user bob"));
    assert_eq!(
        history
            .with(|store| store.expand_bang_request("sudo !!"))
            .ok()
            .flatten()
            .as_deref(),
        Some("sudo user bob")
    );

    shell.clear();
    assert_eq!(
        history
            .with(|store| store.prune_for(5, store.shell_prefix().as_deref()))
            .expect("history lock should be available")
            .expect("prune should succeed"),
        0
    );
    assert_eq!(
        history
            .with(|store| store
                .clear_matching_for("*", store.shell_prefix().as_deref(), false, true)
                .map(|entries| entries.len()))
            .expect("history lock should be available")
            .expect("clear should succeed"),
        4
    );
    assert_eq!(
        history
            .with(|store| store
                .clear_matching_for("*", store.shell_prefix().as_deref(), false, true)
                .map(|entries| entries.len()))
            .expect("history lock should be available")
            .expect("clear should succeed"),
        0
    );
    let persisted = std::fs::read_to_string(&path).expect("history should persist");
    assert_eq!(persisted.lines().count(), 0, "{persisted}");

    let disabled = SharedHistory::new(HistoryConfig {
        enabled: false,
        ..history_config()
    });
    assert_eq!(
        disabled
            .with(|store| store.prune_for(1, store.shell_prefix().as_deref()))
            .expect("history lock should be available")
            .expect("disabled prune is a no-op"),
        0
    );
    assert_eq!(apply_shell_prefix("  ", Some("ldap ")), "");
    assert_eq!(strip_shell_prefix("  ", Some("ldap ")), "");
}

fn make_temp_dir(prefix: &str) -> crate::tests::TestTempDir {
    crate::tests::make_temp_dir(prefix)
}
