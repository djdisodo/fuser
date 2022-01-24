use super::fuse3_sys::{
    fuse_session_destroy, fuse_session_fd, fuse_session_mount, fuse_session_new,
    fuse_session_unmount,
};
use super::{ensure_last_os_error, with_fuse_args, MountOption};
use std::{
    ffi::{c_void, CString},
    fs::File,
    io,
    path::Path,
    ptr,
    sync::Arc,
};
use std::ffi::OsString;

#[derive(Debug)]
pub struct Mount {
    fuse_session: *mut c_void,
}
impl Mount {
    pub fn new(mnt: &Path, options: &[MountOption]) -> io::Result<(Arc<File>, Mount)> {
        #[cfg(unix)] let mnt = CString::new(mnt.as_os_str().as_bytes()).unwrap();
        #[cfg(windows)] let mnt = CString::new(mnt.to_string_lossy().as_bytes()).unwrap();
        with_fuse_args(options, |args| {
            let fuse_session = unsafe { fuse_session_new(args, ptr::null(), 0, ptr::null_mut()) };
            if fuse_session.is_null() {
                return Err(io::Error::last_os_error());
            }
            let mount = Mount { fuse_session };
            let result = unsafe { fuse_session_mount(mount.fuse_session, mnt.as_ptr()) };
            if result != 0 {
                return Err(ensure_last_os_error());
            }
            let fd = unsafe { fuse_session_fd(mount.fuse_session) };
            if fd < 0 {
                return Err(io::Error::last_os_error());
            }
            #[cfg(unix)] {
                use std::os::unix::io::FromRawFd;
                // We dup the fd here as the existing fd is owned by the fuse_session, and we
                // don't want it being closed out from under us:
                let handle = unsafe { libc::dup(fd) };
                if fd < 0 {
                    return Err(io::Error::last_os_error());
                }
                let file = unsafe { File::from_raw_fd(fd) };
                Ok((Arc::new(file), mount))
            }
            #[cfg(windows)] {
                use windows::Win32::Foundation::PWSTR;
                use windows::Win32::Storage::FileSystem::GetFinalPathNameByHandleW;
                use std::fs::OpenOptions;
                use std::os::windows::ffi::OsStringExt;
                use std::mem::transmute;
                use windows::Win32::Foundation::HANDLE;

                let handle = unsafe { libc::get_osfhandle(fd) };
                let mut path_name_buf = vec![0u16; 512];
                let ptr = PWSTR(path_name_buf.as_mut_ptr());
                let read = unsafe { GetFinalPathNameByHandleW(transmute::<_, HANDLE>(handle), ptr, 512, 0) };
                if read == 0 {
                    return Err(io::Error::last_os_error());
                }
                let path = OsString::from_wide(&path_name_buf[..(read as usize)]);
                let file = OpenOptions::new().read(true).write(true).open(path)?;
                Ok((Arc::new(file), mount))
            }
        })
    }
}
impl Drop for Mount {
    fn drop(&mut self) {
        unsafe {
            fuse_session_unmount(self.fuse_session);
            fuse_session_destroy(self.fuse_session);
        }
    }
}
unsafe impl Send for Mount {}
