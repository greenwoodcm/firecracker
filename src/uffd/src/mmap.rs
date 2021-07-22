use std::cmp;
use std::convert::From;
use std::fs::File;
use std::iter::IntoIterator;
use std::os::unix::io::AsRawFd;
use std::ptr;
use std::rc::Rc;
use std::result;

use logger::{IncMetric, METRICS};

use crate::{Error as UffdError, Event, Uffd, UFFD_PAGEFAULT_FLAG_WRITE};

// TODO: Improve this mod since it's a bit crappy and also looks a bit crappy ATM. For example,
// even though this was supposed to be a mmap-based fault handler, I ended up using it mostly
// via MmapUffd::with_regions. If we ever consider uffds useful, I'll refactor this into a handler
// struct that uses generic backends or smt. Also get the logger dependency out.

#[derive(Debug)]
pub enum Error {
    AddressNotFound,
    Mmap,
    Uffd(UffdError),
}

impl From<UffdError> for Error {
    fn from(e: UffdError) -> Self {
        Error::Uffd(e)
    }
}

pub type Result<T> = result::Result<T, Error>;

pub struct Range {
    addr: u64,
    file: Rc<File>,
    offset: i64,
    len: usize,
}

impl Range {
    pub fn new(addr: u64, file: Rc<File>, offset: i64, len: usize) -> Self {
        Range {
            addr,
            file,
            offset,
            len,
        }
    }
}

struct InnerRange {
    start: u64,
    end: u64,
    mmap_addr: u64,
}

impl InnerRange {
    fn new(start: u64, end: u64, mmap_addr: u64) -> Self {
        InnerRange {
            start,
            end,
            mmap_addr,
        }
    }

    fn with_range(r: &Range) -> Result<Self> {
        let mmap_addr = unsafe {
            libc::mmap(
                ptr::null_mut(),
                r.len,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_PRIVATE | libc::MAP_NORESERVE,
                r.file.as_raw_fd(),
                r.offset,
            )
        };

        if mmap_addr == libc::MAP_FAILED {
            return Err(Error::Mmap);
        }

        Ok(InnerRange {
            start: r.addr,
            end: r.addr + r.len as u64,
            mmap_addr: mmap_addr as u64,
        })
    }
}

pub struct MmapUffd {
    ranges: Vec<InnerRange>,
    uffd: Uffd,
    pseudo_page_size: u64,
}

impl MmapUffd {
    // (addr, ptr, len)
    pub unsafe fn with_regions(regions: &[(u64, u64, u64)], pseudo_page_size: u64) -> Result<Self> {
        let uffd = Uffd::new()?;
        let ranges = regions
            .iter()
            .map(|&(addr, ptr, len)| {
                uffd.register(addr, len)?;
                Ok(InnerRange::new(addr, addr + len, ptr))
            })
            .collect::<Result<Vec<_>>>()?;

        Ok(MmapUffd {
            ranges,
            uffd,
            pseudo_page_size,
        })
    }

    pub unsafe fn with_ranges<'a, I: IntoIterator<Item = &'a Range>>(
        ranges: I,
        pseudo_page_size: u64,
    ) -> Result<Self> {
        let uffd = Uffd::new()?;
        let inner_ranges = ranges
            .into_iter()
            .map(|r| {
                let inner = InnerRange::with_range(r)?;
                // This is what makes the function unsafe. Tell more about why.
                uffd.register(inner.start, inner.end - inner.start)?;
                Ok(inner)
            })
            .collect::<Result<Vec<_>>>()?;

        Ok(MmapUffd {
            ranges: inner_ranges,
            uffd,
            pseudo_page_size,
        })
    }

    // TODO: Is address always page aligned? Seems to be.
    fn handle_fault(&mut self, address: u64, flags: u64) -> Result<()> {
        METRICS.uffd.total_faults.inc();

        if flags & u64::from(UFFD_PAGEFAULT_FLAG_WRITE) != 0 {
            METRICS.uffd.write_faults.inc();
        }

        for r in self.ranges.iter() {
            if r.start <= address && r.end > address {
                let pseudo_addr = address & !(self.pseudo_page_size - 1);
                let pseudo_end = pseudo_addr + self.pseudo_page_size;

                let (offset, dst) = if pseudo_addr >= r.start {
                    (pseudo_addr - r.start, pseudo_addr)
                } else {
                    (0, r.start)
                };

                let len = cmp::min(pseudo_end, r.end) - dst;

                // Safe because ...
                unsafe { self.uffd.copy(r.mmap_addr + offset, dst, len) }?;

                return Ok(());
            }
        }

        unsafe { libc::_exit(126) }
        // Err(Error::AddressNotFound)
    }

    pub fn handle_next(&mut self) -> Result<()> {
        match self.uffd.read()? {
            Event::Fault { address, flags } => self.handle_fault(address, flags),
        }
    }
}
