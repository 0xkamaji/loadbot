use std::{
    ffi::OsStr,
    io::{Read, Write},
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};

use anyhow::{Context, Result};
use portable_pty::{ChildKiller, CommandBuilder, PtySize, native_pty_system};

use super::{Control, Event, OperationId, ProcessId};

/// A backend-created program specification for a real interactive terminal.
///
/// This is intentionally a Rust API rather than a serializable GUI contract:
/// adapters may expose opaque launch capabilities, but never program/argv.
#[derive(Clone)]
pub struct InteractiveCommand {
    command: CommandBuilder,
}

impl InteractiveCommand {
    pub fn new(program: impl AsRef<OsStr>) -> Self {
        Self {
            command: CommandBuilder::new(program),
        }
    }

    pub fn arg(&mut self, argument: impl AsRef<OsStr>) -> &mut Self {
        self.command.arg(argument);
        self
    }

    pub fn args<I, S>(&mut self, arguments: I) -> &mut Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        self.command.args(arguments);
        self
    }

    pub fn current_dir(&mut self, directory: impl AsRef<Path>) -> &mut Self {
        self.command.cwd(directory.as_ref());
        self
    }

    pub fn env(&mut self, key: impl AsRef<OsStr>, value: impl AsRef<OsStr>) -> &mut Self {
        self.command.env(key, value);
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InteractiveExitStatus {
    pub code: u32,
    pub signal: Option<String>,
}

impl InteractiveExitStatus {
    pub fn success(&self) -> bool {
        self.code == 0 && self.signal.is_none()
    }
}

impl From<portable_pty::ExitStatus> for InteractiveExitStatus {
    fn from(status: portable_pty::ExitStatus) -> Self {
        Self {
            code: status.exit_code(),
            signal: status.signal().map(str::to_owned),
        }
    }
}

/// Ordered terminal output and completion for one session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InteractiveSessionEvent {
    Output {
        process_id: ProcessId,
        bytes: Vec<u8>,
    },
    Exited {
        process_id: ProcessId,
        status: InteractiveExitStatus,
        cancelled: bool,
    },
    Failed {
        process_id: ProcessId,
        diagnostic: String,
    },
}

struct SessionInner {
    process_id: ProcessId,
    pid: Option<u32>,
    writer: Mutex<Option<Box<dyn Write + Send>>>,
    killer: Mutex<Option<Box<dyn ChildKiller + Send + Sync>>>,
    finished: AtomicBool,
    cancellation_requested: AtomicBool,
    external_handles: AtomicUsize,
}

impl Drop for SessionInner {
    fn drop(&mut self) {
        if !self.finished.load(Ordering::Acquire)
            && let Ok(killer) = self.killer.get_mut()
            && let Some(killer) = killer.as_mut()
        {
            let _ = killer.kill();
        }
    }
}

/// A cloneable control handle for one child attached to a platform PTY.
/// Multiple independently owned handles may coexist in an adapter registry.
pub struct InteractiveSession {
    inner: Arc<SessionInner>,
}

impl Clone for InteractiveSession {
    fn clone(&self) -> Self {
        self.inner.external_handles.fetch_add(1, Ordering::Relaxed);
        Self {
            inner: self.inner.clone(),
        }
    }
}

impl Drop for InteractiveSession {
    fn drop(&mut self) {
        if self.inner.external_handles.fetch_sub(1, Ordering::AcqRel) == 1
            && !self.inner.finished.load(Ordering::Acquire)
            && let Ok(mut killer) = self.inner.killer.lock()
            && let Some(killer) = killer.as_mut()
        {
            self.inner
                .cancellation_requested
                .store(true, Ordering::Release);
            let _ = killer.kill();
        }
    }
}

impl InteractiveSession {
    pub fn start(
        command: InteractiveCommand,
        control: &Control,
        operation_id: OperationId,
        observer: Arc<dyn Fn(InteractiveSessionEvent) + Send + Sync>,
    ) -> Result<Self> {
        control.cancellation.check()?;
        let process_id = ProcessId::random();
        // `native_pty_system` is a POSIX PTY on Unix and a headless ConPTY on
        // Windows; it does not launch a separate external console window.
        let pair = native_pty_system()
            .openpty(PtySize {
                rows: 24,
                cols: 80,
                pixel_width: 0,
                pixel_height: 0,
            })
            .context("could not open interactive pseudo-terminal")?;
        let mut reader = pair
            .master
            .try_clone_reader()
            .context("could not open interactive output")?;
        let writer = pair
            .master
            .take_writer()
            .context("could not open interactive input")?;
        let mut child = pair
            .slave
            .spawn_command(command.command)
            .context("could not spawn interactive process")?;
        drop(pair.slave);

        let pid = child.process_id();
        let inner = Arc::new(SessionInner {
            process_id,
            pid,
            writer: Mutex::new(Some(writer)),
            killer: Mutex::new(Some(child.clone_killer())),
            finished: AtomicBool::new(false),
            cancellation_requested: AtomicBool::new(false),
            external_handles: AtomicUsize::new(1),
        });
        control.emit(Event::InteractiveStarted {
            operation_id,
            process_id,
            pid,
        });

        let thread_inner = inner.clone();
        let thread_control = control.clone();
        std::thread::Builder::new()
            .name(format!("loadbot-pty-{}", process_id.0))
            .spawn(move || {
                let mut buffer = [0_u8; 8192];
                loop {
                    match reader.read(&mut buffer) {
                        Ok(0) => break,
                        Ok(count) => observer(InteractiveSessionEvent::Output {
                            process_id,
                            bytes: buffer[..count].to_vec(),
                        }),
                        Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                        Err(error) => {
                            let diagnostic = format!("could not read interactive output: {error}");
                            observer(InteractiveSessionEvent::Failed {
                                process_id,
                                diagnostic: diagnostic.clone(),
                            });
                            thread_control.emit(Event::Failed {
                                operation_id,
                                process_id,
                                diagnostic,
                            });
                            break;
                        }
                    }
                }

                let result = child
                    .wait()
                    .context("could not wait for interactive process");
                thread_inner.finished.store(true, Ordering::Release);
                if let Ok(mut writer) = thread_inner.writer.lock() {
                    writer.take();
                }
                if let Ok(mut killer) = thread_inner.killer.lock() {
                    killer.take();
                }
                match result {
                    Ok(status) => {
                        let status = InteractiveExitStatus::from(status);
                        let cancelled = thread_inner.cancellation_requested.load(Ordering::Acquire);
                        thread_control.emit(Event::InteractiveExited {
                            operation_id,
                            process_id,
                            status: status.clone(),
                        });
                        if cancelled {
                            thread_control.emit(Event::InteractiveCancelled {
                                operation_id,
                                process_id,
                            });
                        }
                        observer(InteractiveSessionEvent::Exited {
                            process_id,
                            status,
                            cancelled,
                        });
                    }
                    Err(error) => {
                        let diagnostic = format!("{error:#}");
                        observer(InteractiveSessionEvent::Failed {
                            process_id,
                            diagnostic: diagnostic.clone(),
                        });
                        thread_control.emit(Event::Failed {
                            operation_id,
                            process_id,
                            diagnostic,
                        });
                    }
                }
            })
            .context("could not start interactive process observer")?;

        Ok(Self { inner })
    }

    pub fn process_id(&self) -> ProcessId {
        self.inner.process_id
    }

    pub fn os_process_id(&self) -> Option<u32> {
        self.inner.pid
    }

    pub fn is_finished(&self) -> bool {
        self.inner.finished.load(Ordering::Acquire)
    }

    /// Write opaque bytes to the terminal. The bytes are neither retained nor emitted.
    pub fn send_input(&self, input: &[u8]) -> Result<()> {
        if self.is_finished() {
            anyhow::bail!("interactive process has exited");
        }
        let mut writer = self
            .inner
            .writer
            .lock()
            .map_err(|_| anyhow::anyhow!("interactive input lock is unavailable"))?;
        let writer = writer
            .as_mut()
            .context("interactive process input is closed")?;
        writer
            .write_all(input)
            .context("could not write interactive input")?;
        writer.flush().context("could not flush interactive input")
    }

    pub fn terminate(&self) -> Result<()> {
        if self.is_finished() {
            return Ok(());
        }
        self.inner
            .cancellation_requested
            .store(true, Ordering::Release);
        let mut killer =
            self.inner.killer.lock().map_err(|_| {
                anyhow::anyhow!("interactive process termination lock is unavailable")
            })?;
        if let Some(killer) = killer.as_mut() {
            #[cfg(windows)]
            {
                // portable-pty 0.9's WinChildKiller reports a successful
                // TerminateProcess call as an io::Error. The subsequent exit
                // event remains the authoritative completion signal.
                let _ = killer.kill();
            }
            #[cfg(not(windows))]
            killer
                .kill()
                .context("could not terminate interactive process")?;
        }
        Ok(())
    }
}

#[cfg(all(test, unix))]
mod tests {
    use std::{
        sync::{Arc, Mutex, mpsc},
        time::Duration,
    };

    use super::*;
    use crate::process::{ExecutionPolicy, Mode};

    fn shell(script: &str) -> InteractiveCommand {
        let mut command = InteractiveCommand::new("sh");
        command.args(["-c", script]);
        command
    }

    fn start(
        script: &str,
        control: &Control,
    ) -> (InteractiveSession, mpsc::Receiver<InteractiveSessionEvent>) {
        let (sender, receiver) = mpsc::channel();
        let session = InteractiveSession::start(
            shell(script),
            control,
            OperationId::random(),
            Arc::new(move |event| {
                let _ = sender.send(event);
            }),
        )
        .unwrap();
        (session, receiver)
    }

    fn output_until_exit(
        receiver: &mpsc::Receiver<InteractiveSessionEvent>,
    ) -> (Vec<u8>, InteractiveExitStatus, bool) {
        let mut output = Vec::new();
        loop {
            match receiver.recv_timeout(Duration::from_secs(5)).unwrap() {
                InteractiveSessionEvent::Output { bytes, .. } => output.extend(bytes),
                InteractiveSessionEvent::Exited {
                    status, cancelled, ..
                } => return (output, status, cancelled),
                InteractiveSessionEvent::Failed { diagnostic, .. } => panic!("{diagnostic}"),
            }
        }
    }

    #[test]
    fn starts_streams_output_accepts_input_and_reports_exit() {
        let control = Control::default();
        let (session, receiver) = start(
            "printf 'ready\\n'; IFS= read -r value; printf 'reply:%s\\n' \"$value\"",
            &control,
        );
        let first = receiver.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(matches!(first, InteractiveSessionEvent::Output { .. }));
        session.send_input(b"hello-session\n").unwrap();
        let (mut output, status, cancelled) = output_until_exit(&receiver);
        if let InteractiveSessionEvent::Output { bytes, .. } = first {
            output.splice(0..0, bytes);
        }
        let output = String::from_utf8_lossy(&output);
        assert!(output.contains("ready"));
        assert!(output.contains("reply:hello-session"));
        assert!(status.success());
        assert_eq!(status.code, 0);
        assert!(!cancelled);
        assert!(session.is_finished());
        assert_ne!(session.process_id(), ProcessId::default());
    }

    #[test]
    fn terminates_a_running_session_and_reports_cancellation() {
        let (session, receiver) = start("printf 'ready\\n'; sleep 30", &Control::default());
        receiver.recv_timeout(Duration::from_secs(5)).unwrap();
        session.terminate().unwrap();
        let (_, status, cancelled) = output_until_exit(&receiver);
        assert!(cancelled);
        assert!(!status.success());
        assert!(session.is_finished());
    }

    #[test]
    fn stdin_is_never_copied_to_process_observer_events() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let observed = events.clone();
        let control = Control {
            observer: Some(Arc::new(move |event| observed.lock().unwrap().push(event))),
            ..Control::default()
        };
        let (session, receiver) = start("IFS= read -r value; printf 'accepted\\n'", &control);
        session.send_input(b"private-input-value\n").unwrap();
        let _ = output_until_exit(&receiver);
        let debug = format!("{:?}", events.lock().unwrap());
        assert!(!debug.contains("private-input-value"));
        assert!(debug.contains("InteractiveStarted"));
        assert!(debug.contains("InteractiveExited"));
    }

    #[test]
    fn background_capture_behavior_is_unchanged() {
        let mut command = std::process::Command::new("sh");
        command.args(["-c", "printf background; printf diagnostic >&2"]);
        let output = super::super::execute(
            &mut command,
            Mode::Capture { limit: 1024 },
            &Control {
                policy: ExecutionPolicy::Background,
                ..Control::default()
            },
            OperationId::random(),
        )
        .unwrap();
        assert_eq!(output.stdout, b"background");
        assert_eq!(output.stderr, b"diagnostic");
    }
}

#[cfg(all(test, windows))]
mod windows_tests {
    use std::{sync::Arc, sync::mpsc, time::Duration};

    use super::*;

    /// Run this ignored test on a Windows workstation while watching for any
    /// external console flash. It exercises native ConPTY input, output, exit,
    /// and termination without exposing a demo launcher to the GUI.
    #[test]
    #[ignore = "manual native Windows ConPTY smoke test"]
    fn windows_conpty_session_is_headless_interactive_and_terminable() {
        let (sender, receiver) = mpsc::channel();
        let mut command = InteractiveCommand::new("cmd.exe");
        command.args([
            "/D",
            "/Q",
            "/V:ON",
            "/C",
            "set /p VALUE=prompt: & echo reply:!VALUE!",
        ]);
        let session = InteractiveSession::start(
            command,
            &Control::default(),
            OperationId::random(),
            Arc::new(move |event| {
                let _ = sender.send(event);
            }),
        )
        .unwrap();
        session.send_input(b"windows-smoke\r\n").unwrap();
        let mut output = Vec::new();
        loop {
            match receiver.recv_timeout(Duration::from_secs(10)).unwrap() {
                InteractiveSessionEvent::Output { bytes, .. } => output.extend(bytes),
                InteractiveSessionEvent::Exited { status, .. } => {
                    assert!(status.success());
                    break;
                }
                InteractiveSessionEvent::Failed { diagnostic, .. } => panic!("{diagnostic}"),
            }
        }
        assert!(String::from_utf8_lossy(&output).contains("reply:windows-smoke"));

        let (sender, receiver) = mpsc::channel();
        let mut command = InteractiveCommand::new("cmd.exe");
        command.args(["/D", "/Q", "/C", "ping -n 31 127.0.0.1 >nul"]);
        let session = InteractiveSession::start(
            command,
            &Control::default(),
            OperationId::random(),
            Arc::new(move |event| {
                let _ = sender.send(event);
            }),
        )
        .unwrap();
        session.terminate().unwrap();
        loop {
            match receiver.recv_timeout(Duration::from_secs(10)).unwrap() {
                InteractiveSessionEvent::Exited { cancelled, .. } => {
                    assert!(cancelled);
                    break;
                }
                InteractiveSessionEvent::Failed { diagnostic, .. } => panic!("{diagnostic}"),
                InteractiveSessionEvent::Output { .. } => {}
            }
        }
    }
}
