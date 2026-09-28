use doris::player_log::should_log;

#[test]
fn keeps_mpv_relevant_lines() {
    assert!(should_log("vo: gpu-next"));
    assert!(should_log("AO: [pulse] 48000Hz"));
    assert!(should_log("AV: 00:01:02 / 00:10:00"));
    assert!(should_log("hwdec=vulkan"));
    assert!(should_log("Video: h264 1920x1080"));
    assert!(should_log("cache: 45%"));
    assert!(should_log("Exiting... (Quit)"));
    assert!(should_log("resume playback"));
}

#[test]
fn drops_unrelated_lines() {
    assert!(!should_log("this is ordinary chatter"));
    assert!(!should_log("MPV: some already-prefixed line without keywords"));
}

#[test]
fn is_case_insensitive_and_trims() {
    assert!(should_log("  VO: libmpv  "));
    assert!(should_log("Using VAAPI decoder"));
}

#[test]
fn empty_line_is_dropped() {
    assert!(!should_log(""));
    assert!(!should_log("   "));
}
