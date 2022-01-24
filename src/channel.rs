use std::{fs::File, io, sync::Arc};
#[cfg(unix)]
use std::os::unix::prelude::AsRawFd;
#[cfg(windows)]
use std::{os::windows::io::AsRawHandle, ptr::null_mut, mem::transmute};
#[cfg(windows)]
use windows::Win32::Foundation::HANDLE;

use libc::{c_int, c_void, size_t};
use zerocopy::AsBytes;

use crate::reply::ReplySender;

/// A raw communication channel to the FUSE kernel driver
#[derive(Debug)]
pub struct Channel(Arc<File>);

impl Channel {
    /// Create a new communication channel to the kernel driver by mounting the
    /// given path. The kernel driver will delegate filesystem operations of
    /// the given path to the channel.
    pub(crate) fn new(device: Arc<File>) -> Self {
        Self(device)
    }

    /// Receives data up to the capacity of the given buffer (can block).
    pub fn receive(&self, buffer: &mut [u8]) -> io::Result<usize> {
        let rc = unsafe {
            let buf_ptr = buffer.as_mut_ptr() as *mut c_void;
            #[cfg(unix)] libc::read(
                self.0.as_raw_fd(),
                buf_ptr,
                buffer.len() as _,
            );
            #[cfg(windows)] {
                let mut read: u32 = 0;
                if windows::Win32::Storage::FileSystem::ReadFile(
                    unsafe { transmute::<_, HANDLE>(self.0.as_raw_handle()) },
                    buf_ptr,
                    buffer.len() as _,
                    &mut read as _,
                    null_mut()
                ).as_bool() {
                    read as isize
                } else {
                    -1
                }
            }
        };
        if rc < 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(rc as usize)
        }
    }

    /// Returns a sender object for this channel. The sender object can be
    /// used to send to the channel. Multiple sender objects can be used
    /// and they can safely be sent to other threads.
    pub fn sender(&self) -> ChannelSender {
        // Since write/writev syscalls are threadsafe, we can simply create
        // a sender by using the same file and use it in other threads.
        ChannelSender(self.0.clone())
    }
}

#[derive(Clone, Debug)]
pub struct ChannelSender(Arc<File>);

impl ReplySender for ChannelSender {
    fn send(&self, bufs: &[io::IoSlice<'_>]) -> io::Result<()> {
        let rc = unsafe {
            #[cfg(unix)] {
                libc::writev(
                    self.0.as_raw_fd(),
                    bufs.as_ptr() as *const libc::iovec,
                    bufs.len() as c_int,
                )
            }

            #[cfg(windows)] {
                //i wasn't able to find clear way to handle this
                use std::io::Write;
                use windows::Win32::Storage::FileSystem::WriteFile;

                let mut len = 0;
                for x in bufs {
                    len += x.len();
                }
                let mut buf: Vec<u8> = Vec::with_capacity(len);
                for x in bufs {
                    buf.write_all(x.as_bytes())?;
                }
                let mut wrote: u32 = 0;
                if WriteFile(
                    unsafe { transmute::<_, HANDLE>(self.0.as_raw_handle()) },
                    buf.as_ptr() as *const c_void,
                    buf.len() as u32,
                    &mut wrote as _,
                    null_mut()
                ).as_bool() {
                    wrote as isize
                } else {
                    -1
                }
            }
        };
        if rc < 0 {
            Err(io::Error::last_os_error())
        } else {
            debug_assert_eq!(bufs.iter().map(|b| b.len()).sum::<usize>(), rc as usize);
            Ok(())
        }
    }
}
