use doris::ui::widgets::graph::render_sparkline;

#[test]
fn test_braille_empty_history_renders_blank_dots() {
    let out = render_sparkline(&[], 5, "braille");
    assert_eq!(out.chars().count(), 5);
    // All-zero history should render the "no dots raised" braille cell.
    assert!(out.chars().all(|c| c == '\u{2800}'));
}

#[test]
fn test_braille_full_history_renders_full_dots() {
    let history = vec![1.0; 20];
    let out = render_sparkline(&history, 5, "braille");
    assert_eq!(out.chars().count(), 5);
    // Every sample at max should raise all 8 dots in every cell: U+28FF.
    assert!(out.chars().all(|c| c == '\u{28FF}'));
}

#[test]
fn test_braille_uses_two_samples_per_character() {
    // 3 visible columns need up to 6 samples; fewer than that pads with
    // zeros on the left so the most recent samples stay right-aligned.
    let history = vec![1.0, 1.0];
    let out = render_sparkline(&history, 3, "braille");
    assert_eq!(out.chars().count(), 3);
    let chars: Vec<char> = out.chars().collect();
    // First two characters come from padding (zero, zero) and (zero, zero).
    assert_eq!(chars[0], '\u{2800}');
    // Last character is the real (1.0, 1.0) pair -> fully raised.
    assert_eq!(chars[2], '\u{28FF}');
}

#[test]
fn test_block_mode_width_and_range() {
    let history = vec![0.0, 0.5, 1.0];
    let out = render_sparkline(&history, 3, "block");
    assert_eq!(out.chars().count(), 3);
    let chars: Vec<char> = out.chars().collect();
    assert_eq!(chars[0], ' '); // 0.0 -> blank
    assert_eq!(chars[2], '\u{2588}'); // 1.0 -> full block
}

#[test]
fn test_ascii_mode_is_tty_safe() {
    let history = vec![0.0, 1.0];
    let out = render_sparkline(&history, 2, "dot");
    assert_eq!(out.chars().count(), 2);
    // Every character must be plain ASCII -- this mode exists specifically
    // for terminals that can't render Unicode block/braille glyphs.
    assert!(out.is_ascii());
}

#[test]
fn test_unknown_symbol_set_falls_back_to_braille() {
    let out_default = render_sparkline(&[1.0; 4], 2, "not-a-real-mode");
    let out_braille = render_sparkline(&[1.0; 4], 2, "braille");
    assert_eq!(out_default, out_braille);
}

#[test]
fn test_zero_width_returns_empty_string() {
    assert_eq!(render_sparkline(&[0.5, 0.5], 0, "braille"), "");
}

#[test]
fn test_values_outside_0_1_are_clamped() {
    // Should not panic and should behave like the clamped equivalent.
    let out = render_sparkline(&[-5.0, 50.0], 1, "block");
    let clamped = render_sparkline(&[0.0, 1.0], 1, "block");
    assert_eq!(out, clamped);
}
