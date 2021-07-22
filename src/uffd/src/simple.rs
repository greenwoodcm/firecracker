use std::cmp;
use std::convert::From;
use std::result;

use crate::{Error as UffdError, Event, Uffd};

// Simple page fault handler for the uffd example binary.

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

struct InnerRange {
    start: u64,
    end: u64,
}

impl InnerRange {
    fn new(start: u64, end: u64) -> Self {
        InnerRange { start, end }
    }
}

pub struct SimpleUffd {
    ranges: Vec<InnerRange>,
    uffd: Uffd,
    use_zeropage: bool,
    buf: Vec<u8>,
    pseudo_page_size: u64,
}

impl SimpleUffd {
    // (addr, len)
    pub unsafe fn with_regions(
        regions: &[(u64, u64)],
        pseudo_page_size: usize,
        use_zeropage: bool,
    ) -> Result<Self> {
        let uffd = Uffd::new()?;
        let ranges = regions
            .iter()
            .map(|&(addr, len)| {
                uffd.register(addr, len)?;
                Ok(InnerRange::new(addr, addr + len))
            })
            .collect::<Result<Vec<_>>>()?;

        let buf = if use_zeropage {
            Vec::new()
        } else {
            vec![123u8; pseudo_page_size]
        };

        Ok(SimpleUffd {
            ranges,
            uffd,
            use_zeropage,
            buf,
            pseudo_page_size: pseudo_page_size as u64,
        })
    }

    // TODO: Is address always page aligned? Seems to be.
    fn handle_fault(&mut self, address: u64, _flags: u64) -> Result<()> {
        for r in self.ranges.iter() {
            if r.start <= address && r.end > address {
                let pseudo_addr = address & !(self.pseudo_page_size - 1);
                let pseudo_end = pseudo_addr + self.pseudo_page_size;

                let dst = if pseudo_addr >= r.start {
                    pseudo_addr
                } else {
                    r.start
                };

                let len = cmp::min(pseudo_end, r.end) - dst;

                if self.use_zeropage {
                    // Safe because ...
                    unsafe { self.uffd.zeropage(dst, len) }?;
                } else {
                    unsafe { self.uffd.copy(self.buf.as_ptr() as u64, dst, len) }?;
                }

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
