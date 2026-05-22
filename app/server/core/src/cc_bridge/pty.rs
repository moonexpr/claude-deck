/// portable-pty host for cc-bridge.
///
/// `PtySession` wraps a `portable_pty` child + master pair and exposes:
///   - `write_stdin(bytes)` — feed bytes to the pty master (child stdin)
///   - `resize(cols, rows)` — resize the pty
///   - `signal(sig)` — send a Unix signal by name ("INT" / "TERM" / "KILL")
///   - `wait_exit() -> i32` — block until child exits, return exit code
///   - `read_loop(sender)` — background task: read pty output → tokio channel

use std::collections::HashMap;
use std::io::Write;
use std::sync::{Arc, Mutex};

use portable_pty::{native_pty_system, CommandBuilder, PtyPair, PtySize};
use tokio::sync::mpsc;

// Re-export for use in mod.rs
pub use portable_pty::ExitStatus;

// ---------------------------------------------------------------------------
// PtySession
// ---------------------------------------------------------------------------

pub struct PtySession {
    pair: PtyPair,
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
}

impl PtySession {
    /// Spawn a child process in a new PTY.
    ///
    /// `cmd` – first element is the executable, remainder are argv.
    /// `cwd` – working directory for the child.
    /// `env` – extra env vars to inject (merged on top of inherited env).
    /// `cols` / `rows` – initial terminal size.
    pub fn spawn(
        cmd: &[String],
        cwd: &str,
        env: &HashMap<String, String>,
        cols: u16,
        rows: u16,
    ) -> Result<(Self, Box<dyn portable_pty::Child + Send + Sync>), Box<dyn std::error::Error + Send + Sync>> {
        if cmd.is_empty() {
            return Err("cmd must not be empty".into());
        }

        let pty_system = native_pty_system();
        let size = PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        };
        let pair = pty_system.openpty(size)?;

        let mut builder = CommandBuilder::new(&cmd[0]);
        for arg in &cmd[1..] {
            builder.arg(arg);
        }
        builder.cwd(cwd);
        for (k, v) in env {
            builder.env(k, v);
        }

        let child = pair.slave.spawn_command(builder)?;
        let writer = Arc::new(Mutex::new(pair.master.take_writer()?));

        Ok((PtySession { pair, writer }, child))
    }

    /// Write raw bytes to the pty master (→ child stdin).
    pub fn write_stdin(&self, data: &[u8]) -> std::io::Result<()> {
        self.writer.lock().unwrap().write_all(data)
    }

    /// Resize the pty.
    pub fn resize(&self, cols: u16, rows: u16) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.pair.master.resize(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })?;
        Ok(())
    }

    /// Take the pty master reader so the caller can drive the read loop.
    pub fn take_reader(
        &self,
    ) -> Result<Box<dyn std::io::Read + Send>, Box<dyn std::error::Error + Send + Sync>> {
        Ok(self.pair.master.try_clone_reader()?)
    }
}

// ---------------------------------------------------------------------------
// Background read loop
// ---------------------------------------------------------------------------

/// Spawn a blocking thread that reads from `reader` and sends chunks over
/// `tx`. Returns a `JoinHandle` so the caller can clean up.
pub fn spawn_read_loop(
    reader: Box<dyn std::io::Read + Send + 'static>,
    tx: mpsc::Sender<Vec<u8>>,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let mut reader = reader;
        let mut buf = [0u8; 4096];
        loop {
            match reader.read(&mut buf) {
                Ok(0) => break, // EOF — child exited or pty closed
                Ok(n) => {
                    let chunk = buf[..n].to_vec();
                    if tx.blocking_send(chunk).is_err() {
                        break; // WS handler gone
                    }
                }
                Err(_) => break,
            }
        }
    })
}

// Bring Read into scope for spawn_read_loop
use std::io::Read;

// ---------------------------------------------------------------------------
// Signal helpers
// ---------------------------------------------------------------------------

/// Send a signal to a process by PID.  Only meaningful on Unix.
/// `sig` is one of "INT", "TERM", "KILL" (case-insensitive).
#[cfg(unix)]
pub fn send_signal(pid: u32, sig: &str) {
    use std::os::raw::c_int;
    let signum: c_int = match sig.to_uppercase().as_str() {
        "INT" => libc::SIGINT,
        "TERM" => libc::SIGTERM,
        "KILL" => libc::SIGKILL,
        _ => return,
    };
    unsafe {
        libc::kill(pid as libc::pid_t, signum);
    }
}

#[cfg(not(unix))]
pub fn send_signal(_pid: u32, _sig: &str) {
    // No-op on non-Unix
}
