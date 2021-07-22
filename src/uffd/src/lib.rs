mod gen;
pub mod mmap;
pub mod simple;

use std::convert::TryInto;
use std::fs::File;
use std::io::Read;
use std::mem;
use std::os::unix::io::FromRawFd;
use std::result;

use libc::{syscall, SYS_userfaultfd};

use utils::ioctl::ioctl_with_mut_ref;

use gen::{
    uffd_msg, uffdio_api, uffdio_copy, uffdio_range, uffdio_register, uffdio_zeropage, UFFDIO_API,
    UFFDIO_COPY, UFFDIO_REGISTER, UFFDIO_REGISTER_MODE_MISSING, UFFDIO_ZEROPAGE, UFFD_API,
    UFFD_EVENT_PAGEFAULT, UFFD_FEATURE_MISSING_HUGETLBFS, UFFD_FEATURE_MISSING_SHMEM, _UFFDIO_API,
    _UFFDIO_REGISTER, _UFFDIO_UNREGISTER,
};

pub use gen::{UFFD_PAGEFAULT_FLAG_WRITE, _UFFDIO_COPY, _UFFDIO_ZEROPAGE};

const UFFD_MSG_SIZE: usize = mem::size_of::<uffd_msg>();

#[derive(Debug)]
pub enum Error {
    IntoRawFd,
    InvalidEvent,
    IoctlApi,
    IoctlCopy,
    IoctlRegister,
    IoctlZeropage,
    Read,
    Syscall,
}

pub type Result<T> = result::Result<T, Error>;

#[derive(Debug)]
pub enum Event {
    Fault { address: u64, flags: u64 },
}

pub struct Uffd {
    file: File,
    // Reading one message at a time for now.
    buf: [u8; UFFD_MSG_SIZE],
}

impl Uffd {
    /*
    fn check_register_ioctls(ioctls: u64) {
        assert_ne!(ioctls & (1 << _UFFDIO_COPY as u64), 0);
        // This won't be available with hugepages.
        // assert_ne!(ioctls & (1 << uffd_gen::_UFFDIO_ZEROPAGE as u64), 0);
    }
    */

    pub fn new() -> Result<Self> {
        // Safe because we check the return value.
        let fd = unsafe { syscall(SYS_userfaultfd, 0) };

        if fd == -1 {
            return Err(Error::Syscall);
        }

        // Safe because we got a valid fd from the `userfaultfd` syscall.
        let file = unsafe { File::from_raw_fd(fd.try_into().or(Err(Error::IntoRawFd))?) };

        let mut api = uffdio_api {
            api: UFFD_API,
            // TODO: UFFD_FEATURE_MISSING_SHMEM doesn't appear to do anything. Is that so?
            features: u64::from(UFFD_FEATURE_MISSING_HUGETLBFS | UFFD_FEATURE_MISSING_SHMEM),
            ioctls: 0,
        };

        // Safe because we are passing valid parameters, and are checking the return value.
        if unsafe { ioctl_with_mut_ref(&file, UFFDIO_API(), &mut api) } == -1 {
            return Err(Error::IoctlApi);
        }

        assert_ne!(api.features & u64::from(UFFD_FEATURE_MISSING_SHMEM), 0);

        assert_ne!(api.ioctls & (1 << u64::from(_UFFDIO_API)), 0);
        assert_ne!(api.ioctls & (1 << u64::from(_UFFDIO_REGISTER)), 0);
        assert_ne!(api.ioctls & (1 << u64::from(_UFFDIO_UNREGISTER)), 0);

        Ok(Uffd {
            file,
            buf: [0u8; UFFD_MSG_SIZE],
        })
    }

    pub fn read(&mut self) -> Result<Event> {
        self.file.read(self.buf.as_mut()).map_err(|_| Error::Read)?;

        let msg_ptr = self.buf.as_ptr() as *const uffd_msg;
        // Safe because the previous read succeeded, and thus we have a uffd_msg in the
        // memory area held by self.buf.
        let msg = unsafe { &*msg_ptr };

        match u32::from(msg.event) {
            UFFD_EVENT_PAGEFAULT => {
                // Safe because the event type is "page fault".
                let fault = unsafe { &msg.arg.pagefault };
                Ok(Event::Fault {
                    address: fault.address,
                    flags: fault.flags,
                })
            }
            _ => Err(Error::InvalidEvent),
        }
    }

    pub unsafe fn register(&self, start: u64, len: u64) -> Result<u64> {
        let mut register = uffdio_register {
            range: uffdio_range { start, len },
            mode: UFFDIO_REGISTER_MODE_MISSING,
            ioctls: 0,
        };

        if ioctl_with_mut_ref(&self.file, UFFDIO_REGISTER(), &mut register) == -1 {
            return Err(Error::IoctlRegister);
        }

        Ok(register.ioctls)
    }

    // TODO: Ensure meaningful error conditions are handler for this and `copy`. For example, it
    // seems like an error might be returned if the fault has been resolved already. Dunno if
    // that's relevant for now, but better stay on the safe side.
    pub unsafe fn zeropage(&self, start: u64, len: u64) -> Result<()> {
        let mut zeropage = uffdio_zeropage {
            range: uffdio_range { start, len },
            mode: 0,
            zeropage: 0,
        };

        if ioctl_with_mut_ref(&self.file, UFFDIO_ZEROPAGE(), &mut zeropage) == -1 {
            return Err(Error::IoctlZeropage);
        }

        Ok(())
    }

    pub unsafe fn copy(&self, src: u64, dst: u64, len: u64) -> Result<()> {
        let mut copy = uffdio_copy {
            dst,
            src,
            len,
            mode: 0,
            copy: 0,
        };

        if ioctl_with_mut_ref(&self.file, UFFDIO_COPY(), &mut copy) == -1 {
            return Err(Error::IoctlCopy);
        }

        Ok(())
    }
}
