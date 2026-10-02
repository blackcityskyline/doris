use doris::transmission::Download;

/// The arithmetic every status column is built on, pinned without a
/// daemon.
///
/// These are the numbers a user reads rather than computes, so each one
/// has a case where the naive reading is wrong: a ratio before anything
/// has been downloaded, an ETA when Transmission has stopped predicting
/// one, and a percentage past 100 from a resumed file.
fn row() -> Download {
    Download {
        id: 1,
        hash: "045e85f2ebc24a875a64fe2e9ac9b61f7aad0499".into(),
        name: "Oblivion (2013) [1080p]".into(),
        fraction: 0.25,
        download_speed: 4_200_000,
        upload_speed: 890_000,
        peers: 12,
        seeds: 3,
        total_size: 2_000_000_000,
        left: 1_500_000_000,
        dir: "/home/u/Downloads".into(),
        status: 4,
        uploaded: 340_000_000,
        downloaded: 500_000_000,
        eta: 359,
        ..Download::default()
    }
}

/// A ratio before anything is downloaded has no answer. `0.00` would look
/// like a measurement, and a panel showing a measured zero for a torrent
/// that just started is lying about it.
#[test]
fn test_the_ratio_is_absent_until_something_has_been_downloaded() {
    let mut d = row();
    d.downloaded = 0;
    assert_eq!(d.ratio(), None);
    d.downloaded = 1_000;
    d.uploaded = 500;
    assert_eq!(d.ratio(), Some(0.5));
}

/// Transmission reports a fraction; a person reads a percentage.
#[test]
fn test_progress_is_a_percentage_and_stays_inside_the_range() {
    assert_eq!(row().percent(), 25.0);
    let mut d = row();
    d.fraction = 1.5;
    assert_eq!(d.percent(), 100.0, "a resumed file reports over one");
    d.fraction = -1.0;
    assert_eq!(d.percent(), 0.0);
}

/// `-1` means Transmission is not predicting one, which is not the same as
/// "no time left". Reporting `done` for a stalled torrent is how a panel
/// says something false with a straight face.
#[test]
fn test_an_unpredictable_eta_is_absent_and_a_finished_one_is_done() {
    let mut d = row();
    d.eta = -1;
    assert_eq!(d.eta_text(), None);
    d.eta = 0;
    d.left = 0;
    assert_eq!(d.eta_text().as_deref(), Some("done"));
    d.left = 900_000;
    assert_eq!(d.eta_text().as_deref(), Some("878.9 KB left"));
    d.eta = 359;
    assert_eq!(d.eta_text().as_deref(), Some("5m left"));
    d.eta = 3700;
    assert_eq!(d.eta_text().as_deref(), Some("1h 1m left"));
    d.eta = 3600;
    assert_eq!(
        d.eta_text().as_deref(),
        Some("1h left"),
        "\"1h 0m\" is noise"
    );
    d.eta = 176_400;
    assert_eq!(d.eta_text().as_deref(), Some("2d 1h left"));
    d.eta = 172_800;
    assert_eq!(
        d.eta_text().as_deref(),
        Some("2d left"),
        "and no empty hour after it"
    );
    d.eta = 90_000;
    assert_eq!(
        d.eta_text().as_deref(),
        Some("1d 1h left"),
        "90000 seconds is a day and an hour, not two days"
    );
}

/// The status code is Transmission's contract; the word is ours, and the
/// TUI and the CLI have to say the same thing about the same number.
#[test]
fn test_every_status_code_has_a_word() {
    for (code, word) in [
        (0, "paused"),
        (1, "queued to verify"),
        (2, "verifying"),
        (3, "queued to download"),
        (4, "downloading"),
        (5, "queued to seed"),
        (6, "seeding"),
    ] {
        let mut d = row();
        d.status = code;
        assert_eq!(d.state(), word, "status {code}");
    }
    // Anything past the last known code is still seeding rather than blank,
    // because a blank state column reads as "nothing is happening".
    let mut d = row();
    d.status = 99;
    assert_eq!(d.state(), "seeding");
}

/// Seeders across trackers are summed, and a tracker that has not answered
/// counts as zero rather than poisoning the sum.
#[test]
fn test_seeds_are_summed_across_trackers() {
    use doris::transmission::TrackerStat;
    let mut d = row();
    d.trackers = vec![
        TrackerStat {
            seeders: 10,
            ..TrackerStat::default()
        },
        TrackerStat {
            seeders: 4,
            ..TrackerStat::default()
        },
    ];
    assert_eq!(d.tracker_seeds(), 14);
}
