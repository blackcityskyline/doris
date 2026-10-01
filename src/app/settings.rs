//! Reading the config, writing it back, and reporting what happened.
//!
//! `persist_config` runs after every settings change rather than only on a
//! clean exit: "save config on exit" governs a final flush, not whether
//! changes are remembered at all.

use super::*;

impl App {
    /// Move `config.preset_index` one step and apply the spec it lands
    /// on. Both `Shift+P` and the Options "Presets" row come through
    /// here, so there is one place that decides what a preset cycle is.
    pub(super) fn cycle_layout_preset(&mut self, direction: i8) {
        if self.config.disable_presets || self.config.presets.is_empty() {
            return;
        }
        self.config.preset_index = cycle_index(
            self.config.preset_index,
            self.config.presets.len(),
            direction,
        );
        let spec = self.config.presets[self.config.preset_index].clone();
        self.ui.zones.apply_preset(&spec);
    }

    /// See the free function of the same name for the resolution logic
    /// and why this exists; this just supplies `&self.config`/`self.args`.
    pub(super) fn resolve_cookie_file(&self) -> Option<std::path::PathBuf> {
        resolve_cookie_file(&self.config, self.args.cookie_file.as_deref())
    }

    /// Write the config back now rather than at exit.
    ///
    /// Every in-app edit of `self.config` goes through here: the
    /// Settings modal always did this, and the Trackers checkboxes only
    /// flipped a field in memory -- with "Save config on exit" defaulting
    /// to off, a checked source simply evaporated when the app closed.
    /// The path is `--config`, the one `config::load` read from, so a
    /// run pointed at another file cannot be overwritten by (or overwrite
    /// the contents of) the default one. A failure is worth saying out
    /// loud in the Log zone: silently losing a setting is what this
    /// function exists to stop.
    pub(super) fn persist_config(&mut self) {
        if let Err(e) = crate::config::save(&self.config, self.args.config.as_deref()) {
            self.ui.add_log(&format!("Failed to save config: {e}"));
        }
    }

    /// A line the user must not miss: the Log zone, the full log `L`
    /// opens, and the file on disk. TorrServer's failures used to reach
    /// only the last of the three, so the panel said nothing while the
    /// app already knew the answer.
    pub(super) fn report(&mut self, module: &str, msg: &str) {
        self.ui.add_log(msg);
        self.ui.add_detail(msg);
        crate::log::log(module, msg);
    }

    /// Turning TorrServer on says out loud whether it is actually up.
    ///
    /// The switch used to invert a bool and stop there: with the systemd
    /// unit stopped nothing happened until a stream was started, and the
    /// reason lived in the file log. So the moment it is enabled the
    /// server is pinged; if it does not answer, the `systemctl` that
    /// would start it gets one unprivileged chance -- never with a
    /// password, which cannot be answered from here and would only hang
    /// or fail silently -- and whatever it said is reported verbatim.
    pub(super) async fn check_torrserver_on_enable(&mut self) {
        let url = self.torrserver.base_url().to_string();
        let started = if self.torrserver.is_reachable().await {
            None
        } else {
            Some(
                match std::process::Command::new("systemctl")
                    .args(["start", "torrserver.service"])
                    .output()
                {
                    Ok(out) if out.status.success() => Ok(String::new()),
                    Ok(out) => {
                        let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
                        // An empty stderr would read as "no reason given";
                        // the exit status still says how it failed.
                        Err(if err.is_empty() {
                            out.status.to_string()
                        } else {
                            err
                        })
                    }
                    Err(e) => Err(e.to_string()),
                },
            )
        };
        let msg = torrserver_enable_message(&url, started);
        self.report("torrserver", &msg);
    }
}
