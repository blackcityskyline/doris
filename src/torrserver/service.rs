//! Starting and stopping the TorrServer doris streams through.
//!
//! TorrServer is a server, and a server that dies with the program that
//! started it is a library. So the process is started detached -- its own
//! session, no terminal, no stdin -- and it stays up after doris exits,
//! which is what the setting says.
//!
//! Two rules about stopping, because this is a process on someone's machine:
//!
//! doris only ever kills a process it started. The pid is written when it
//! starts and read back when it stops; no pid file, no kill. A TorrServer
//! started by systemd is not doris's to stop, and `enable_torrserver = false`
//! is about what *doris* does with it, not about what the machine may run.
//!
//! And it is started only when it is not already answering. If something
//! else is already on the port -- the systemd unit, a container -- doris uses
//! that one and says so, rather than fighting it for the port.

use anyhow::{Context, Result};
use std::io::Write;
use std::os::unix::process::CommandExt;
use std::path::PathBuf;

/// Where the pid of the process doris started is kept.
///
/// In doris's own data directory, next to the log: a pid is state, and
/// state that lives beside the program that owns it is findable.
pub fn pid_file() -> PathBuf {
    crate::log::state_dir().join("torrserver.pid")
}

/// The binary to run: an explicit path, or `torrserver` from `PATH`.
pub fn binary(configured: &str) -> Option<String> {
    let wanted = configured.trim();
    if wanted.is_empty() {
        which::which("torrserver")
            .ok()
            .map(|p| p.to_string_lossy().to_string())
    } else {
        let path = PathBuf::from(wanted);
        path.exists().then(|| path.to_string_lossy().to_string())
    }
}

/// Where TorrServer keeps its cache. TorrServer's own default is
/// `./settings` in the working directory, which for a detached process is
/// wherever doris happened to be started from -- so it is set explicitly.
pub fn data_dir(configured: &str) -> PathBuf {
    let wanted = configured.trim();
    if wanted.is_empty() {
        crate::log::state_dir().join("torrserver")
    } else {
        PathBuf::from(wanted)
    }
}

/// The pid of the process doris started, if it is still that process.
///
/// A pid file is a claim, not a fact: the number it holds may since have
/// been reused by something else entirely. Checked against the command line
/// before anything is signalled.
pub fn owned_pid() -> Option<i32> {
    let text = std::fs::read_to_string(pid_file()).ok()?;
    let pid: i32 = text.trim().parse().ok()?;
    is_ours(pid).then_some(pid)
}

/// Whether `pid` is still the TorrServer doris started.
///
/// The name has to *be* TorrServer, not contain it. Found by a test: this
/// crate's own test binary is called
/// `torrserver_service_tests-6f3a…`, a `contains` check claimed it was the
/// server, and the stop rule would have signalled a process that has nothing
/// to do with streaming. A wrapper script named `torrserver.sh` and a
/// `torrserverctl` are the same class of mistake.
fn is_ours(pid: i32) -> bool {
    let Ok(exe) = std::fs::read_link(format!("/proc/{pid}/exe")) else {
        return false; // gone, or not ours to inspect
    };
    let stem = exe
        .file_stem()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    // The capitalisation has changed between releases and a rename is still
    // the same program.
    stem.eq_ignore_ascii_case("torrserver")
}

/// Forget the pid file, whether or not anything was there.
fn clear_pid_file() {
    let _ = std::fs::remove_file(pid_file());
}

/// Start TorrServer, detached, unless something is already answering.
///
/// `reachable` is the caller's own answer for the port: starting a second
/// server on a port that is taken does not fail loudly, it answers the wrong
/// requests.
pub fn start(configured_bin: &str, configured_dir: &str, reachable: bool) -> Result<String> {
    if reachable {
        return Ok("already answering".to_string());
    }
    if let Some(pid) = owned_pid() {
        return Ok(format!(
            "already started here (pid {pid}), not answering yet"
        ));
    }
    // A pid file naming a process that is gone is a claim that has expired.
    // Left in place it makes every later start answer "already started here",
    // for ever, about a server that died on the way up.
    clear_pid_file();

    let bin = binary(configured_bin).context(
        "TorrServer is not on PATH. Put `torrserver_path` in config.toml, or \
         install it -- it is one binary from https://github.com/YouROK/TorrServer",
    )?;
    let dir = data_dir(configured_dir);
    std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;

    // `pre_exec` is an unsafe closure because what runs inside it runs between
    // fork and exec, where a panic would leave the child half-built. The one
    // call here is a syscall that cannot allocate, so it cannot panic.
    let child = unsafe {
        std::process::Command::new(&bin)
            .arg("-d")
            .arg(&dir)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .pre_exec(|| {
                // `setsid(2)`: become a session leader with no terminal.
                // Failing is not fatal -- the process starts either way --
                // and the outcome is checked by whether the port answers.
                setsid();
                Ok(())
            })
            .spawn()
    }
    .with_context(|| format!("running {bin} -d {}", dir.display()))?;

    let pid = child.id() as i32;
    // Written before the wait: if it dies on the way up, the file is stale
    // but honest, and the next start clears it.
    write_pid_file(pid)?;
    Ok(format!("started (pid {pid}), data in {}", dir.display()))
}

/// Stop the process doris started, and only that one.
///
/// Returns what it did, because "stopped" and "there was nothing of mine to
/// stop" are different answers and the caller has to be able to say which.
pub fn stop() -> String {
    let Some(pid) = owned_pid() else {
        clear_pid_file();
        return "nothing of ours to stop -- a TorrServer doris did not start is \
                not stopped"
            .to_string();
    };

    // SIGTERM first: the server closes its cache on the way out, and a kill
    // in the middle of that is how a cache gets truncated.
    let signalled = unsafe { libc_kill(pid, 15) } == 0;
    let mut gone = false;
    for _ in 0..40 {
        // 250 ms a step, ten seconds in all: long enough for a cache to be
        // closed, short enough that nobody thinks the key hung.
        std::thread::sleep(std::time::Duration::from_millis(250));
        if unsafe { libc_kill(pid, 0) } != 0 {
            gone = true;
            break;
        }
    }
    if !gone {
        unsafe { libc_kill(pid, 9) };
        std::thread::sleep(std::time::Duration::from_millis(250));
    }
    clear_pid_file();
    let how = if signalled {
        "stopped"
    } else {
        "had gone already"
    };
    format!("{how} (pid {pid})")
}

/// Whether the process doris started is running right now.
pub fn running() -> Option<i32> {
    owned_pid()
}

fn write_pid_file(pid: i32) -> Result<()> {
    let path = pid_file();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut file =
        std::fs::File::create(&path).with_context(|| format!("writing {}", path.display()))?;
    writeln!(file, "{pid}")?;
    Ok(())
}

/// `kill(2)` without taking a dependency for it.
///
/// `libc` is not on the list and is not worth adding for one signal: the
/// same call through `Command` cannot address a pid by number, and this is
/// the one place the code needs to.
unsafe fn libc_kill(pid: i32, signal: i32) -> i32 {
    extern "C" {
        fn kill(pid: i32, sig: i32) -> i32;
    }
    kill(pid, signal)
}

/// `setsid(2)`, declared here for the same reason as `kill`: one call is
/// not a dependency.
unsafe fn setsid() -> i32 {
    extern "C" {
        fn setsid() -> i32;
    }
    setsid()
}
