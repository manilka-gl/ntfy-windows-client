use std::ffi::c_void;

type Handle = *mut c_void;

#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetCurrentProcess() -> Handle;
    fn SetProcessWorkingSetSize(
        process: Handle,
        minimum_working_set_size: usize,
        maximum_working_set_size: usize,
    ) -> i32;
}

#[link(name = "psapi")]
unsafe extern "system" {
    fn EmptyWorkingSet(process: Handle) -> i32;
}

/// Ask Windows to evict unused resident pages after a transient UI surface has
/// been destroyed. This reduces the tray listener's working set; mapped image
/// pages and private commit remain available and can be faulted back on demand.
pub fn trim_working_set() {
    unsafe {
        let process = GetCurrentProcess();
        let _ = SetProcessWorkingSetSize(process, usize::MAX, usize::MAX);
        let _ = EmptyWorkingSet(process);
    }
}
