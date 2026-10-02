//! The Options modal's descriptor table: every category, every row, and the text on it. It is
//! data, not logic, and it was 290 of the 940 lines of `settings.rs`.

use super::settings::{
    bool_str, streaming_settings_items, welcome_settings_items, SettingsAction, SettingsCategory,
    SettingsItem,
};
use crate::config::Config;

pub(super) fn settings_categories(
    config: &Config,
    browser_hidden: bool,
    mode_str: String,
    theme_str: String,
    preset_display: String,
) -> Vec<SettingsCategory> {
    vec![
        SettingsCategory {
            name: "general".into(),
            items: vec![
                SettingsItem {
                    label: "Color theme".into(),
                    value: theme_str,
                    description: vec![
                        "Set color theme.".into(),
                        "".into(),
                        "Choose from all theme files in".into(),
                        "\"~/.config/doris/themes\".".into(),
                        "".into(),
                        "Use Left/Right to cycle.".into(),
                    ],
                    action: SettingsAction::CycleTheme,
                },
                SettingsItem {
                    label: "Theme background".into(),
                    value: bool_str(config.theme_background),
                    description: vec![
                        "If the theme set background".into(),
                        "should be shown.".into(),
                        "".into(),
                        "Set to False if you want".into(),
                        "terminal background".into(),
                        "transparency.".into(),
                    ],
                    action: SettingsAction::ToggleThemeBackground,
                },
                SettingsItem {
                    label: "Truecolor".into(),
                    value: bool_str(config.truecolor),
                    description: vec![
                        "Sets if 24-bit truecolor".into(),
                        "should be used.".into(),
                        "".into(),
                        "Will convert 24-bit colors to".into(),
                        "256 color if False.".into(),
                        "".into(),
                        "Set to False if your terminal".into(),
                        "doesn't have truecolor".into(),
                        "support.".into(),
                    ],
                    action: SettingsAction::ToggleTruecolor,
                },
                SettingsItem {
                    label: "False tty".into(),
                    value: bool_str(config.false_tty),
                    description: vec![
                        "Force basic 16-color, no-mouse".into(),
                        "TTY-compatible rendering.".into(),
                        "".into(),
                        "Set to True on a real Linux".into(),
                        "console (not a terminal".into(),
                        "emulator) with no 256/true".into(),
                        "color support.".into(),
                    ],
                    action: SettingsAction::ToggleFalseTty,
                },
                SettingsItem {
                    label: "Vim keys".into(),
                    value: bool_str(config.vim_keys),
                    description: vec![
                        "Enable vim keys.".into(),
                        "".into(),
                        "Set to True to enable".into(),
                        "\"j,k\" keys for directional".into(),
                        "control in lists, in addition".into(),
                        "to the arrow keys (which".into(),
                        "always work).".into(),
                    ],
                    action: SettingsAction::ToggleVimKeys,
                },
                SettingsItem {
                    label: "Disable mouse".into(),
                    value: bool_str(config.disable_mouse),
                    description: vec!["Disable all mouse events.".into()],
                    action: SettingsAction::ToggleMouse,
                },
                SettingsItem {
                    label: "Disable presets".into(),
                    value: bool_str(config.disable_presets),
                    description: vec![
                        "Hide the Presets entry below".into(),
                        "and disable cycling through".into(),
                        "saved zone layouts.".into(),
                    ],
                    action: SettingsAction::ToggleDisablePresets,
                },
                SettingsItem {
                    label: "Presets".into(),
                    value: if config.disable_presets {
                        "disabled".into()
                    } else {
                        preset_display
                    },
                    description: vec![
                        "Cycle through saved zone".into(),
                        "layouts (which panels are".into(),
                        "shown).".into(),
                        "".into(),
                        "Edit the `presets` list in".into(),
                        "config.toml to customize.".into(),
                    ],
                    action: SettingsAction::CyclePreset,
                },
                SettingsItem {
                    label: "Show boxes".into(),
                    value: bool_str(config.show_boxes),
                    description: vec![
                        "Show borders around panels.".into(),
                        "".into(),
                        "Set to False for a more".into(),
                        "minimal look with no borders.".into(),
                    ],
                    action: SettingsAction::ToggleShowBoxes,
                },
                SettingsItem {
                    label: "Update ms".into(),
                    value: config.update_ms.to_string(),
                    description: vec![
                        "Torrent panel refresh".into(),
                        "interval, in milliseconds.".into(),
                        "".into(),
                        "Min value: 100 ms".into(),
                        "Max value: 86400000 ms".into(),
                    ],
                    action: SettingsAction::SetUpdateMs,
                },
                SettingsItem {
                    label: "Rounded corners".into(),
                    value: bool_str(config.rounded_corners),
                    description: vec![
                        "Rounded corners on boxes.".into(),
                        "".into(),
                        "Is always False if False tty".into(),
                        "is On.".into(),
                    ],
                    action: SettingsAction::ToggleRoundedCorners,
                },
                SettingsItem {
                    label: "Terminal sync".into(),
                    value: bool_str(config.terminal_sync),
                    description: vec![
                        "Output synchronization.".into(),
                        "".into(),
                        "Use terminal synchronized".into(),
                        "output sequences to reduce".into(),
                        "flickering on supported".into(),
                        "terminals.".into(),
                    ],
                    action: SettingsAction::ToggleTerminalSync,
                },
                SettingsItem {
                    label: "Graph symbol".into(),
                    value: config.graph_symbol.clone(),
                    description: vec![
                        "Symbol set used for graphs".into(),
                        "and sparklines.".into(),
                        "".into(),
                        "\"braille\", \"block\" or \"dot\".".into(),
                    ],
                    action: SettingsAction::CycleGraphSymbol,
                },
                SettingsItem {
                    label: "Health check".into(),
                    value: "press Enter".into(),
                    description: vec![
                        "Run system health check.".into(),
                        "".into(),
                        "Verifies browser, TorrServer,".into(),
                        "saved credentials, cookies,".into(),
                        "and known sources.".into(),
                    ],
                    action: SettingsAction::RunHealthCheck,
                },
                SettingsItem {
                    label: "Save config on exit".into(),
                    value: bool_str(config.save_config_on_exit),
                    description: vec![
                        "Automatically save current".into(),
                        "settings to config.toml on".into(),
                        "exit.".into(),
                    ],
                    action: SettingsAction::ToggleSaveOnExit,
                },
            ],
        },
        SettingsCategory {
            name: "streaming".into(),
            items: streaming_settings_items(config, browser_hidden, &mode_str),
        },
        SettingsCategory {
            name: "download".into(),
            items: vec![
                SettingsItem {
                    label: "Enable downloading".into(),
                    value: bool_str(config.download_enabled),
                    description: vec![
                        "Allow \"Download\" play mode".into(),
                        "(saving .torrent files)".into(),
                        "in addition to streaming.".into(),
                    ],
                    action: SettingsAction::ToggleDownloadEnabled,
                },
                SettingsItem {
                    label: "Downloads directory".into(),
                    value: config.download_dir_mode.clone(),
                    description: vec![
                        "Which directory slot is".into(),
                        "active: \"default\" (OS".into(),
                        "Downloads folder) or".into(),
                        "custom1/2/3 below.".into(),
                    ],
                    action: SettingsAction::CycleDownloadDirMode,
                },
                SettingsItem {
                    label: "Custom directory 1".into(),
                    value: if config.download_dir_custom_1.is_empty() {
                        "(not set)".into()
                    } else {
                        config.download_dir_custom_1.clone()
                    },
                    description: vec![
                        "Edit `download_dir_custom_1`".into(),
                        "in config.toml.".into(),
                        "".into(),
                        "An in-app path editor is".into(),
                        "planned; not yet built.".into(),
                    ],
                    action: SettingsAction::Close,
                },
                SettingsItem {
                    label: "Custom directory 2".into(),
                    value: if config.download_dir_custom_2.is_empty() {
                        "(not set)".into()
                    } else {
                        config.download_dir_custom_2.clone()
                    },
                    description: vec![
                        "Edit `download_dir_custom_2`".into(),
                        "in config.toml.".into(),
                    ],
                    action: SettingsAction::Close,
                },
                SettingsItem {
                    label: "Custom directory 3".into(),
                    value: if config.download_dir_custom_3.is_empty() {
                        "(not set)".into()
                    } else {
                        config.download_dir_custom_3.clone()
                    },
                    description: vec![
                        "Edit `download_dir_custom_3`".into(),
                        "in config.toml.".into(),
                    ],
                    action: SettingsAction::Close,
                },
                SettingsItem {
                    label: "Close torrent core on exit".into(),
                    value: bool_str(config.close_torrent_core_on_exit),
                    description: vec![
                        "Stop the download when Doris".into(),
                        "exits, instead of leaving it".into(),
                        "running on the server.".into(),
                        "".into(),
                        "The torrent is paused, not".into(),
                        "removed: it stays on disk and".into(),
                        "resumes when asked for again.".into(),
                    ],
                    action: SettingsAction::ToggleCloseTorrentCoreOnExit,
                },
                SettingsItem {
                    label: "Open detailed log".into(),
                    value: "L".into(),
                    description: vec![
                        "Toggle detailed log view.".into(),
                        "".into(),
                        "Shows detailed application logs".into(),
                        "for debugging purposes.".into(),
                    ],
                    action: SettingsAction::OpenLog,
                },
            ],
        },
        SettingsCategory {
            name: "welcome".into(),
            items: welcome_settings_items(config),
        },
    ]
}
