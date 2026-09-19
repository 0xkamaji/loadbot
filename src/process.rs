//! Synchronous, bounded process execution. Streaming is not a terminal adapter.
use std::io::Read;
use std::process::{Command, ExitStatus, Output, Stdio};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc,
};
use std::time::Duration;

use anyhow::{Context, Result};

thread_local! {
    static CURRENT: std::cell::RefCell<Control> = std::cell::RefCell::new(Control::default());
}
/// A synchronous operation lends its control to nested Git inspection helpers.
/// The previous value is restored on return/unwind; it never crosses threads.
pub(crate) struct Scope(Control, std::marker::PhantomData<std::rc::Rc<()>>);
pub(crate) fn scope(control: &Control) -> Scope {
    Scope(
        CURRENT.with(|current| current.replace(control.clone())),
        std::marker::PhantomData,
    )
}
pub(crate) fn current_control() -> Control {
    CURRENT.with(|current| current.borrow().clone())
}
/// Finish short post-mutation inspection so a completed step can be reported.
pub(crate) fn critical_scope() -> Scope {
    let mut control = current_control();
    control.cancellation = Cancellation::default();
    scope(&control)
}
impl Drop for Scope {
    fn drop(&mut self) {
        CURRENT.with(|current| {
            current.replace(self.0.clone());
        });
    }
}

#[derive(Clone, Default)]
pub struct Cancellation(Arc<AtomicBool>);
impl Cancellation {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Release);
    }
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
    pub fn check(&self) -> Result<()> {
        if self.is_cancelled() {
            Err(Cancelled.into())
        } else {
            Ok(())
        }
    }
}
#[derive(Debug)]
pub struct Cancelled;
impl std::fmt::Display for Cancelled {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("operation cancelled")
    }
}
impl std::error::Error for Cancelled {}

#[derive(Debug)]
pub struct CleanupIncomplete;
impl std::fmt::Display for CleanupIncomplete {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("process cleanup could not be verified; cancellation is not complete and the checkout must be retained")
    }
}
impl std::error::Error for CleanupIncomplete {}

#[derive(Debug)]
pub struct TimedOut(pub Duration);
impl std::fmt::Display for TimedOut {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "process did not finish within {} seconds",
            self.0.as_secs()
        )
    }
}
impl std::error::Error for TimedOut {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stream {
    Stdout,
    Stderr,
}
#[derive(Debug, Clone)]
pub enum Event {
    OperationStarted,
    OperationFinished {
        outcome: crate::interaction::OperationStatus,
        partial: bool,
    },
    Starting {
        program: std::ffi::OsString,
        arguments: Vec<std::ffi::OsString>,
        directory: Option<std::path::PathBuf>,
    },
    Started {
        pid: u32,
    },
    Output {
        stream: Stream,
        bytes: Vec<u8>,
    },
    Exited {
        pid: u32,
        status: ExitStatus,
    },
    Cancelled {
        pid: u32,
    },
    Failed {
        diagnostic: String,
    },
}
/// Callbacks run synchronously on the executor thread and must return promptly.
/// Output is not retained here; consumers choose their own bounded storage.
#[derive(Clone)]
pub struct Control {
    pub cancellation: Cancellation,
    pub observer: Option<Arc<dyn Fn(Event) + Send + Sync>>,
    /// CLI Git may prompt via /dev/tty despite captured stdout. GUI callers
    /// must disable terminal access and use noninteractive credentials.
    pub terminal: bool,
}
impl Default for Control {
    fn default() -> Self {
        Self {
            cancellation: Cancellation::default(),
            observer: None,
            terminal: true,
        }
    }
}
impl Control {
    pub fn emit(&self, event: Event) {
        if let Some(observer) = &self.observer {
            observer(event);
        }
    }
}
#[derive(Debug, Clone, Copy)]
pub enum Mode {
    Inherit,
    Stream,
    /// Machine-readable Git queries must fail, not parse silently truncated data.
    Capture {
        limit: usize,
    },
}

pub fn execute(command: &mut Command, mode: Mode, control: &Control) -> Result<Output> {
    execute_bounded(command, mode, control, None)
}

pub fn execute_with_timeout(
    command: &mut Command,
    mode: Mode,
    control: &Control,
    timeout: Duration,
) -> Result<Output> {
    execute_bounded(command, mode, control, Some(timeout))
}

fn execute_bounded(
    command: &mut Command,
    mode: Mode,
    control: &Control,
    timeout: Option<Duration>,
) -> Result<Output> {
    control.cancellation.check()?;
    control.emit(Event::Starting {
        program: command.get_program().to_owned(),
        arguments: command.get_args().map(std::ffi::OsStr::to_owned).collect(),
        directory: command.get_current_dir().map(std::path::Path::to_owned),
    });
    control.cancellation.check()?;
    let result = execute_inner(command, mode, control, timeout);
    if let Err(error) = &result
        && error.downcast_ref::<Cancelled>().is_none()
    {
        control.emit(Event::Failed {
            diagnostic: format!("{error:#}"),
        });
    }
    result
}

fn execute_inner(
    command: &mut Command,
    mode: Mode,
    control: &Control,
    timeout: Option<Duration>,
) -> Result<Output> {
    let piped = !matches!(mode, Mode::Inherit);
    if piped {
        command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
    } else {
        command
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit());
    }
    platform::prepare(command);
    let mut child = ReapedChild(command.spawn().context("could not spawn process")?);
    let pid = child.id();
    let owner = match platform::Owner::new(
        &child,
        !piped || (control.terminal && matches!(mode, Mode::Capture { .. })),
    ) {
        Ok(owner) => owner,
        Err(error) => {
            child
                .kill()
                .context("process ownership failed and child could not be stopped")?;
            child
                .wait()
                .context("process ownership failed and child could not be reaped")?;
            return Err(error);
        }
    };
    control.emit(Event::Started { pid });
    let (sender, receiver) = mpsc::sync_channel(16);
    let reader_stop = Arc::new(AtomicBool::new(false));
    let mut readers = Vec::new();
    if piped {
        readers.push(drain(
            child.stdout.take().unwrap(),
            Stream::Stdout,
            sender.clone(),
            reader_stop.clone(),
        )?);
        readers.push(drain(
            child.stderr.take().unwrap(),
            Stream::Stderr,
            sender.clone(),
            reader_stop.clone(),
        )?);
    }
    drop(sender);
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let mut overflow = false;
    let mut read_error = None;
    let mut cancelled = false;
    let mut status = None;
    let mut closed = !piped;
    let mut stopped = false;
    let mut cleanup_incomplete = false;
    let mut cleanup_started = None;
    let started_at = std::time::Instant::now();
    let mut timed_out = false;
    loop {
        if status.is_none() {
            status = child
                .try_wait()
                .context("could not inspect process status")?;
        }
        if !stopped && status.is_none() && control.cancellation.is_cancelled() {
            owner.terminate().context(CleanupIncomplete)?;
            stopped = true;
            cancelled = true;
            cleanup_started = Some(std::time::Instant::now());
        }
        if !stopped
            && status.is_none()
            && timeout.is_some_and(|timeout| started_at.elapsed() >= timeout)
        {
            owner.terminate().context(CleanupIncomplete)?;
            stopped = true;
            timed_out = true;
            cleanup_started = Some(std::time::Instant::now());
        }
        if status.is_some() && !stopped {
            // A child retaining a pipe must not keep the reader threads alive forever.
            owner.terminate().context(CleanupIncomplete)?;
            stopped = true;
            cleanup_started = Some(std::time::Instant::now());
        }
        if cleanup_started.is_some_and(|start| start.elapsed() > Duration::from_secs(5)) && !closed
        {
            reader_stop.store(true, Ordering::Release);
            cleanup_incomplete = true;
            read_error = Some(std::io::Error::other(
                "descendant retained output pipes after process termination; cleanup could not be verified",
            ));
        }
        if status.is_some() && closed {
            break;
        }
        match receiver.recv_timeout(Duration::from_millis(10)) {
            Ok((stream, Ok(bytes))) => {
                if let Mode::Capture { limit } = mode {
                    let buffer = if stream == Stream::Stdout {
                        &mut stdout
                    } else {
                        &mut stderr
                    };
                    let room = limit.saturating_sub(buffer.len());
                    buffer.extend_from_slice(&bytes[..bytes.len().min(room)]);
                    overflow |= bytes.len() > room;
                }
                control.emit(Event::Output { stream, bytes });
            }
            Ok((_, Err(error))) => read_error = Some(error),
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                closed = true;
                if status.is_none() {
                    std::thread::sleep(Duration::from_millis(10));
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
    }
    for reader in readers {
        reader
            .join()
            .map_err(|_| anyhow::anyhow!("process output reader panicked"))?;
    }
    let status = status.unwrap();
    owner.finish().context(CleanupIncomplete)?;
    control.emit(Event::Exited { pid, status });
    if cleanup_incomplete {
        return Err(CleanupIncomplete.into());
    }
    if let Some(error) = read_error {
        return Err(error).context("could not read process output or verify cleanup");
    }
    if cancelled {
        control.emit(Event::Cancelled { pid });
        return Err(Cancelled.into());
    }
    if timed_out {
        return Err(TimedOut(timeout.expect("timeout exists after timeout branch")).into());
    }
    if overflow {
        anyhow::bail!(
            "process output exceeded capture limit; streamed output remains available to the observer"
        );
    }
    Ok(Output {
        status,
        stdout,
        stderr,
    })
}

struct ReapedChild(std::process::Child);
impl std::ops::Deref for ReapedChild {
    type Target = std::process::Child;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl std::ops::DerefMut for ReapedChild {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}
impl Drop for ReapedChild {
    fn drop(&mut self) {
        if !matches!(self.0.try_wait(), Ok(Some(_))) {
            let _ = self.0.kill();
        }
        let _ = self.0.wait();
    }
}

type Chunk = (Stream, std::io::Result<Vec<u8>>);
#[cfg(unix)]
trait Pipe: Read + std::os::fd::AsRawFd + Send + 'static {}
#[cfg(unix)]
impl<T: Read + std::os::fd::AsRawFd + Send + 'static> Pipe for T {}
#[cfg(windows)]
trait Pipe: Read + std::os::windows::io::AsRawHandle + Send + 'static {}
#[cfg(windows)]
impl<T: Read + std::os::windows::io::AsRawHandle + Send + 'static> Pipe for T {}

fn drain(
    mut pipe: impl Pipe,
    stream: Stream,
    sender: mpsc::SyncSender<Chunk>,
    stop: Arc<AtomicBool>,
) -> Result<std::thread::JoinHandle<()>> {
    #[cfg(unix)]
    unsafe {
        let flags = libc::fcntl(pipe.as_raw_fd(), libc::F_GETFL);
        if flags == -1
            || libc::fcntl(pipe.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK) == -1
        {
            return Err(std::io::Error::last_os_error().into());
        }
    }
    Ok(std::thread::spawn(move || {
        let mut buffer = [0; 8192];
        loop {
            if stop.load(Ordering::Acquire) {
                break;
            }
            #[cfg(windows)]
            unsafe {
                let mut available = 0;
                if windows_sys::Win32::System::Pipes::PeekNamedPipe(
                    pipe.as_raw_handle(),
                    std::ptr::null_mut(),
                    0,
                    std::ptr::null_mut(),
                    &mut available,
                    std::ptr::null_mut(),
                ) == 0
                {
                    let error = std::io::Error::last_os_error();
                    if error.raw_os_error()
                        != Some(windows_sys::Win32::Foundation::ERROR_BROKEN_PIPE as i32)
                    {
                        let _ = sender.send((stream, Err(error)));
                    }
                    break;
                }
                if available == 0 {
                    std::thread::sleep(Duration::from_millis(10));
                    continue;
                }
            }
            match pipe.read(&mut buffer) {
                Ok(0) => break,
                Ok(count) => {
                    if sender.send((stream, Ok(buffer[..count].to_vec()))).is_err() {
                        break;
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(10))
                }
                Err(error) => {
                    let _ = sender.send((stream, Err(error)));
                    break;
                }
            }
        }
    }))
}

#[cfg(unix)]
mod platform {
    use super::*;
    use std::os::unix::process::CommandExt;
    pub fn prepare(command: &mut Command) {
        command.process_group(0);
    }
    pub struct Owner {
        pid: i32,
        foreground: Option<i32>,
        finished: std::cell::Cell<bool>,
    }
    impl Owner {
        pub fn new(child: &std::process::Child, terminal: bool) -> Result<Self> {
            let pid = child.id() as i32;
            let foreground = unsafe { libc::tcgetpgrp(0) };
            let foreground =
                (terminal && foreground == unsafe { libc::getpgrp() }).then_some(foreground);
            let owner = Self {
                pid,
                foreground,
                finished: std::cell::Cell::new(false),
            };
            if foreground.is_some() {
                set_foreground(pid)?;
                unsafe {
                    libc::kill(-pid, libc::SIGCONT);
                }
            }
            Ok(owner)
        }
        pub fn terminate(&self) -> Result<()> {
            if unsafe { libc::kill(-self.pid, libc::SIGKILL) } != 0 {
                let error = std::io::Error::last_os_error();
                if error.raw_os_error() != Some(libc::ESRCH) {
                    return Err(error).context("could not terminate process group");
                }
            }
            Ok(())
        }
        pub fn finish(&self) -> Result<()> {
            let deadline = std::time::Instant::now() + Duration::from_secs(5);
            loop {
                if !group_running(self.pid)? {
                    self.finished.set(true);
                    return Ok(());
                }
                if std::time::Instant::now() >= deadline {
                    anyhow::bail!(
                        "process group {} cleanup is incomplete; cancellation has not completed",
                        self.pid
                    );
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        }
    }
    fn group_running(group: i32) -> Result<bool> {
        if unsafe { libc::kill(-group, 0) } != 0
            && std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH)
        {
            return Ok(false);
        }
        #[cfg(target_os = "linux")]
        {
            // Grandchildren are reaped by their own parent/init. Zombies no longer
            // execute or hold resources; do not mistake them for running children.
            for entry in std::fs::read_dir("/proc")? {
                let entry = entry?;
                if entry.file_name().to_string_lossy().parse::<u32>().is_err() {
                    continue;
                }
                let stat = match std::fs::read_to_string(entry.path().join("stat")) {
                    Ok(stat) => stat,
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                    Err(error) => {
                        return Err(error).context("could not verify process group cleanup");
                    }
                };
                let Some((_, fields)) = stat.rsplit_once(") ") else {
                    continue;
                };
                let fields: Vec<_> = fields.split_whitespace().take(3).collect();
                if fields.len() == 3
                    && fields[2].parse::<i32>() == Ok(group)
                    && !matches!(fields[0], "Z" | "X")
                {
                    return Ok(true);
                }
            }
            Ok(false)
        }
        #[cfg(not(target_os = "linux"))]
        {
            Ok(unsafe { libc::kill(-group, 0) } == 0)
        }
    }
    impl Drop for Owner {
        fn drop(&mut self) {
            if !self.finished.get() {
                let _ = self.terminate();
            }
            if let Some(foreground) = self.foreground {
                let _ = set_foreground(foreground);
            }
        }
    }
    fn set_foreground(group: i32) -> Result<()> {
        // Block SIGTTOU on this thread only while transferring terminal ownership.
        unsafe {
            let mut mask = std::mem::zeroed();
            let mut old = std::mem::zeroed();
            libc::sigemptyset(&mut mask);
            libc::sigaddset(&mut mask, libc::SIGTTOU);
            let result = libc::pthread_sigmask(libc::SIG_BLOCK, &mask, &mut old);
            if result != 0 {
                return Err(std::io::Error::from_raw_os_error(result).into());
            }
            let result = libc::tcsetpgrp(0, group);
            let error = std::io::Error::last_os_error();
            libc::pthread_sigmask(libc::SIG_SETMASK, &old, std::ptr::null_mut());
            if result != 0 {
                return Err(error.into());
            }
        }
        Ok(())
    }
}

#[cfg(windows)]
#[path = "process_windows.rs"]
mod platform;

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn bounded_capture_stops_a_hung_process() {
        let mut command = Command::new("sh");
        command.args(["-c", "sleep 10"]);
        let started = std::time::Instant::now();
        let error = execute_with_timeout(
            &mut command,
            Mode::Capture { limit: 1024 },
            &Control {
                terminal: false,
                ..Control::default()
            },
            Duration::from_millis(50),
        )
        .unwrap_err();
        assert!(error.downcast_ref::<TimedOut>().is_some());
        assert!(started.elapsed() < Duration::from_secs(2));
    }
}
