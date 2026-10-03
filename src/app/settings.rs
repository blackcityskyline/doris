//! Reading the config, writing it back, and reporting what happened.
//!
//! `persist_config` runs after every settings change rather than only on a
//! clean exit: "save config on exit" governs a final flush, not whether
//! changes are remembered at all.

use super::*;

impl App {
    /// Move `config.preset_index` one step and apply the spec it lands on.
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

    pub(super) fn persist_config(&mut self) {
        if let Err(e) = crate::config::save(&self.config, self.args.config.as_deref()) {
            self.ui.add_log(&format!("Failed to save config: {e}"));
        }
    }

    /// Start TorrServer if the setting is on and nothing is answering.
    ///
    /// Separate from [`Self::apply_torrserver_switch`] because startup must
    /// not *stop* anything: a doris that starts while a TorrServer is already
    /// running has no business shutting it down on its way past.
    pub(super) async fn ensure_torrserver(&mut self) {
        use crate::torrserver::service;

        if self.torrserver.is_reachable().await {
            return;
        }
        match service::start(
            &self.config.torrserver_path,
            &self.config.torrserver_data_dir,
            false,
        ) {
            Ok(what) => {
                self.ui.add_log(&format!("TorrServer: {what}"));
                // The port needs a moment; without the wait the first search
                // of the session says "unreachable" about a server that is
                // one second from answering.
                for _ in 0..20 {
                    if self.torrserver.is_reachable().await {
                        self.ui.add_log("TorrServer is up.");
                        return;
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(250)).await;
                }
                // Measured, not guessed: TorrServer takes no port argument.
                // The one it listens on is compiled in, so a `torrserver_url`
                // pointing anywhere else makes doris start a process that
                // cannot bind -- and the honest message is the one that
                // names the reason rather than leaving a corpse in the log.
                self.ui.add_log(&format!(
                    "TorrServer was started but {} is not answering. It takes \
                     no port argument -- the port is compiled in -- so \
                     `torrserver_url` has to be where it actually listens.",
                    self.torrserver.base_url()
                ));
            }
            Err(e) => self.ui.add_log(&format!("TorrServer did not start: {e}")),
        }
    }

    /// What the TorrServer switch does to the process, as opposed to the
    /// config field.
    ///
    /// One function so the two callers -- the Options modal and startup --
    /// cannot disagree about what "on" means.
    pub(super) async fn apply_torrserver_switch(&mut self, wanted: bool) {
        use crate::torrserver::service;

        if wanted {
            let reachable = self.torrserver.is_reachable().await;
            match service::start(
                &self.config.torrserver_path,
                &self.config.torrserver_data_dir,
                reachable,
            ) {
                Ok(what) => self.ui.add_log(&format!("TorrServer: {what}")),
                Err(e) => self.ui.add_log(&format!("TorrServer did not start: {e}")),
            }
            self.check_torrserver_on_enable().await;
        } else {
            let what = service::stop();
            self.ui.add_log(&format!("TorrServer: {what}"));
            self.ui.state = AppState::Idle;
        }
    }

    /// A line the user must not miss: the Log zone, the full log `L` opens, and the file on
    /// disk.
    pub(super) fn report(&mut self, module: &str, msg: &str) {
        self.ui.add_log(msg);
        self.ui.add_detail(msg);
        crate::log::log(module, msg);
    }

    /// Turning TorrServer on says out loud whether it is actually up.
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
