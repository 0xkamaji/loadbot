//! Windows Job Object ownership: assign while suspended, then resume all threads.
use super::*;
use std::os::windows::{io::AsRawHandle, process::CommandExt};
use windows_sys::Win32::{
    Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE},
    System::{
        Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, TH32CS_SNAPTHREAD, THREADENTRY32, Thread32First, Thread32Next,
        },
        JobObjects::{
            AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
            JOBOBJECT_BASIC_ACCOUNTING_INFORMATION, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
            JobObjectBasicAccountingInformation, JobObjectExtendedLimitInformation,
            QueryInformationJobObject, SetInformationJobObject, TerminateJobObject,
        },
        Threading::{CREATE_SUSPENDED, OpenThread, ResumeThread, THREAD_SUSPEND_RESUME},
    },
};

pub fn prepare(command: &mut Command) {
    command.creation_flags(CREATE_SUSPENDED);
}
struct Handle(HANDLE);
impl Drop for Handle {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}
pub struct Owner {
    job: Handle,
}
impl Owner {
    pub fn new(child: &std::process::Child, _terminal: bool) -> Result<Self> {
        unsafe {
            let handle = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            if handle.is_null() {
                return Err(std::io::Error::last_os_error().into());
            }
            let owner = Self {
                job: Handle(handle),
            };
            let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            if SetInformationJobObject(
                handle,
                JobObjectExtendedLimitInformation,
                (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                std::mem::size_of_val(&limits) as u32,
            ) == 0
            {
                return Err(std::io::Error::last_os_error().into());
            }
            if AssignProcessToJobObject(handle, child.as_raw_handle()) == 0 {
                return Err(std::io::Error::last_os_error())
                    .context("could not assign child to cancellation job");
            }
            let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0);
            if snapshot == INVALID_HANDLE_VALUE {
                return Err(std::io::Error::last_os_error().into());
            }
            let snapshot = Handle(snapshot);
            let mut entry: THREADENTRY32 = std::mem::zeroed();
            entry.dwSize = std::mem::size_of_val(&entry) as u32;
            let mut found = false;
            let mut next = Thread32First(snapshot.0, &mut entry);
            while next != 0 {
                if entry.th32OwnerProcessID == child.id() {
                    let thread = OpenThread(THREAD_SUSPEND_RESUME, 0, entry.th32ThreadID);
                    if thread.is_null() {
                        return Err(std::io::Error::last_os_error().into());
                    }
                    let thread = Handle(thread);
                    if ResumeThread(thread.0) == u32::MAX {
                        return Err(std::io::Error::last_os_error().into());
                    }
                    found = true;
                }
                next = Thread32Next(snapshot.0, &mut entry);
            }
            if !found {
                anyhow::bail!("could not find suspended child thread");
            }
            Ok(owner)
        }
    }
    pub fn terminate(&self) -> Result<()> {
        if unsafe { TerminateJobObject(self.job.0, 1) } == 0 {
            return Err(std::io::Error::last_os_error()).context("could not terminate process job");
        }
        Ok(())
    }
    pub fn finish(&self) -> Result<()> {
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        loop {
            let mut info: JOBOBJECT_BASIC_ACCOUNTING_INFORMATION = unsafe { std::mem::zeroed() };
            if unsafe {
                QueryInformationJobObject(
                    self.job.0,
                    JobObjectBasicAccountingInformation,
                    (&mut info as *mut JOBOBJECT_BASIC_ACCOUNTING_INFORMATION).cast(),
                    std::mem::size_of_val(&info) as u32,
                    std::ptr::null_mut(),
                )
            } == 0
            {
                return Err(std::io::Error::last_os_error())
                    .context("could not verify job cleanup");
            }
            if info.ActiveProcesses == 0 {
                return Ok(());
            }
            if std::time::Instant::now() >= deadline {
                anyhow::bail!("process job cleanup is incomplete; cancellation has not completed");
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}
