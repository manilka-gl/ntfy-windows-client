use std::ffi::c_void;

type Handle = *mut c_void;

const QUOTA_LIMITS_HARDWS_MIN_DISABLE: u32 = 0x0000_0002;
const QUOTA_LIMITS_HARDWS_MAX_DISABLE: u32 = 0x0000_0008;

#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetCurrentProcess() -> Handle;
    fn SetProcessWorkingSetSize(
        process: Handle,
        minimum_working_set_size: usize,
        maximum_working_set_size: usize,
    ) -> i32;
    fn SetProcessWorkingSetSizeEx(
        process: Handle,
        minimum_working_set_size: usize,
        maximum_working_set_size: usize,
        flags: u32,
    ) -> i32;
}

#[link(name = "psapi")]
unsafe extern "system" {
    fn EmptyWorkingSet(process: Handle) -> i32;
}

/// Ask Windows to evict unused resident pages after a transient UI surface has
/// been destroyed. Mapped image pages and private commit remain available and
/// can be faulted back on demand. The extended call also disables inherited
/// hard working-set limits before the explicit empty-working-set request.
pub fn trim_working_set() {
    unsafe {
        let process = GetCurrentProcess();
        let flags = QUOTA_LIMITS_HARDWS_MIN_DISABLE | QUOTA_LIMITS_HARDWS_MAX_DISABLE;
        let _ = SetProcessWorkingSetSizeEx(process, usize::MAX, usize::MAX, flags);
        let _ = SetProcessWorkingSetSize(process, usize::MAX, usize::MAX);
        let _ = EmptyWorkingSet(process);
        // A second pass catches pages released by the renderer while the first
        // trim call is unwinding.
        let _ = EmptyWorkingSet(process);
    }
}
