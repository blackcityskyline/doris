use clap::Parser;
use doris::cli::{Args, Command, ConfigCommand, TorrentCommand};
use doris::config::Config;

fn parse(args: &[&str]) -> Args {
    let mut argv = vec!["doris"];
    argv.extend_from_slice(args);
    Args::try_parse_from(argv).expect("parses")
}

#[test]
fn test_no_subcommand_is_the_tui() {
    assert!(parse(&["oblivion"]).resolve_command().is_none());
    assert_eq!(parse(&["oblivion"]).query.as_deref(), Some("oblivion"));
}

/// `--cli` used to mean "search and print", which is what `search` is
/// called now. It has to keep working: scripts exist.
#[test]
fn test_the_old_cli_flag_is_a_search() {
    let args = parse(&["--cli", "oblivion"]);
    match args.resolve_command() {
        Some(Command::Search(a)) => assert_eq!(a.query.as_deref(), Some("oblivion")),
        other => panic!("--cli should be a search, got {other:?}"),
    }
}

#[test]
fn test_search_takes_the_flags_the_results_panel_has() {
    let args = parse(&[
        "search",
        "oblivion",
        "--source",
        "yts",
        "--source",
        "nyaa",
        "--group",
        "movies",
        "--filter",
        "seeds:>50",
        "--pages",
        "3",
        "--limit",
        "10",
    ]);
    let Some(Command::Search(a)) = args.resolve_command() else {
        panic!("expected a search");
    };
    assert_eq!(a.query.as_deref(), Some("oblivion"));
    assert_eq!(a.source, vec!["yts", "nyaa"]);
    assert_eq!(a.group.as_deref(), Some("movies"));
    assert_eq!(a.filter.as_deref(), Some("seeds:>50"));
    assert_eq!(a.pages, 3);
    assert_eq!(a.limit, Some(10));
}

/// An empty query is browse mode, and it has to reach the search that way
/// rather than being turned into "no search".
#[test]
fn test_an_empty_query_is_browse_mode_and_is_still_a_search() {
    let args = parse(&["search"]);
    let Some(Command::Search(a)) = args.resolve_command() else {
        panic!("expected a search");
    };
    assert_eq!(
        a.query, None,
        "absent, not an empty string to be searched for"
    );
    assert_eq!(a.pages, 1);
}

/// `--json` is global: it is on the root, so it works before or after the
/// subcommand name and cannot be forgotten on one of ten of them.
#[test]
fn test_json_is_global_and_survives_either_order() {
    assert!(parse(&["--json", "sources"]).json);
    assert!(parse(&["sources", "--json"]).json);
    assert!(!parse(&["sources"]).json);
}

#[test]
fn test_a_row_is_named_by_magnet_or_by_where_it_sits_in_the_list() {
    let args = parse(&["play", "--magnet", "magnet:?xt=urn:btih:abc"]);
    let Some(Command::Play(a)) = args.resolve_command() else {
        panic!("expected a play");
    };
    assert_eq!(a.magnet.as_deref(), Some("magnet:?xt=urn:btih:abc"));
    assert_eq!(a.index, 0, "and the first row is the default");

    let args = parse(&["download", "oblivion", "--source", "yts", "--index", "3"]);
    let Some(Command::Download(a)) = args.resolve_command() else {
        panic!("expected a download");
    };
    // The query lives in the search flags, not in a second field that
    // would answer to the same `--query`.
    assert_eq!(a.search.query.as_deref(), Some("oblivion"));
    assert_eq!(a.index, 3);
}

#[test]
fn test_the_torrent_subcommands_carry_their_hash() {
    for (args, expected) in [
        (vec!["torrent", "status"], "status"),
        (vec!["torrent", "pause", "abc"], "pause"),
        (vec!["torrent", "resume", "abc"], "resume"),
        (vec!["torrent", "remove", "abc"], "remove"),
    ] {
        let parsed = parse(&args);
        let Some(Command::Torrent(c)) = parsed.resolve_command() else {
            panic!("{args:?} should be a torrent command");
        };
        let got = match &c {
            TorrentCommand::Status { .. } => "status",
            TorrentCommand::Pause { .. } => "pause",
            TorrentCommand::Resume { .. } => "resume",
            TorrentCommand::Remove { .. } => "remove",
            other => panic!("unexpected {other:?}"),
        };
        assert_eq!(got, expected);
    }
}

// --- naming a group ----------------------------------------------------

#[test]
fn test_the_group_names_are_the_categories_the_frame_shows() {
    use doris::app::cli_commands::parse_group;
    for name in ["games", "Movies", "TV", "anime"] {
        assert!(parse_group(name).is_ok_and(|g| g.is_some()), "{name}");
    }
    // `all` is the `◀ all ▶` button, which is the *absence* of a
    // category rather than one of the four.
    assert_eq!(parse_group("all").unwrap(), None);
    assert_eq!(parse_group("ALL").unwrap(), None);
    let err = parse_group("documentaries").unwrap_err().to_string();
    assert!(
        err.contains("games"),
        "the error lists what is valid: {err}"
    );
}

// --- the filter is the one the `f` box speaks --------------------------

#[test]
fn test_the_filter_flag_is_the_results_filter_not_a_second_one() {
    use doris::filter::Filter;
    use doris::sources::models::TorrentItem;
    let row = TorrentItem {
        title: "Oblivion (2013) [1080p]".into(),
        size: "1.85 GB".into(),
        seeds: "100".into(),
        seeds_n: 100,
        source: "yts".into(),
        ..TorrentItem::default()
    };
    let other = TorrentItem {
        source: "rutor".into(),
        ..row.clone()
    };
    let keep = |expr: &str, row: &TorrentItem| Filter::parse(expr).matches(row);
    assert!(
        keep("seeds:>50", &row),
        "the same language the filter box parses"
    );
    assert!(!keep("seeds:>500", &row));
    assert!(keep("src:yts", &row));
    assert!(keep("title:oblivion", &row) && !keep("title:predator", &row));
    assert!(
        !keep("src:yts", &other),
        "and it reads the row, not the command"
    );
}

// --- what a config value becomes ---------------------------------------

#[test]
fn test_a_config_value_keeps_the_type_the_field_already_has() {
    let mut config = Config::default();
    config.set("update_ms", "2500").unwrap();
    assert_eq!(
        config.update_ms, 2500,
        "a number is parsed, not stored as text"
    );

    // Both directions, because "understood" is two arms of one match and
    // a test that only ever writes `no` never reaches the other one.
    config.set("rounded_corners", "no").unwrap();
    assert!(!config.rounded_corners, "and a boolean is understood");
    config.set("rounded_corners", "yes").unwrap();
    assert!(config.rounded_corners);

    config.set("enabled_sources", "yts, nyaa").unwrap();
    assert_eq!(
        config.enabled_sources,
        vec!["yts", "nyaa"],
        "a list splits on commas"
    );

    config.set("torrserver_url", "http://host:1234").unwrap();
    assert_eq!(config.torrserver_url, "http://host:1234");
}

#[test]
fn test_a_value_that_is_not_that_type_is_refused() {
    let mut config = Config::default();
    let err = config
        .set("rounded_corners", "maybe")
        .unwrap_err()
        .to_string();
    assert!(err.contains("boolean"), "{err}");
    assert!(config.set("update_ms", "soon").is_err());
    // A field that is not there is named, not silently created: this is
    // the whole reason `get`/`set` read the serialized form rather than a
    // hand-written table, but it still has to answer that it said no.
    let err = config.get("no_such_setting").unwrap_err().to_string();
    assert!(err.contains("no_such_setting"), "{err}");
    assert!(config.set("no_such_setting", "1").is_err());
}

/// `--cli` with a query used to print a table of results and exit 0 even
/// when nothing was found. A pipeline cannot tell that from success.
#[test]
fn test_a_command_is_named_rather_than_matched_by_string() {
    let parsed = parse(&["search", "x"]);
    assert!(matches!(parsed.resolve_command(), Some(Command::Search(_))));
    let parsed = parse(&["logs"]);
    assert!(matches!(parsed.resolve_command(), Some(Command::Logs)));
    let parsed = parse(&["config", "list"]);
    assert!(matches!(
        parsed.resolve_command(),
        Some(Command::Config(ConfigCommand::List))
    ));
}
