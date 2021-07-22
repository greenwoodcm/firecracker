// #[macro_use]
// extern crate clap;
// extern crate libc;
// extern crate rand;

// extern crate uffd;

use std::fs::File;
use std::process::exit;
use std::ptr;
use std::rc::Rc;
use std::slice;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;
use std::time::Instant;

use clap::{App, Arg};
use rand::seq::SliceRandom;
use rand::thread_rng;

use uffd::mmap::{MmapUffd, Range};
use uffd::simple::SimpleUffd;

static NUM_FAULTS: AtomicUsize = AtomicUsize::new(0);
static NUM_WRITES: AtomicUsize = AtomicUsize::new(0);

fn exit_msg(msg: &str) {
    println!("{}", msg);
    exit(1);
}

fn inc(v: &AtomicUsize) {
    // A single thread will call this function for a particular value at any given time,
    // so no need for fetch_add etc.
    v.store(v.load(Ordering::Relaxed) + 1, Ordering::Relaxed);
}

fn get(v: &AtomicUsize) -> usize {
    v.load(Ordering::Relaxed)
}

fn mmap(size: usize, flags: i32, fd: i32, offset: i64) -> *mut u8 {
    let addr = unsafe {
        libc::mmap(
            ptr::null_mut(),
            size,
            libc::PROT_READ | libc::PROT_WRITE,
            flags,
            fd,
            offset,
        )
    };

    if addr == libc::MAP_FAILED {
        exit_msg("mmap failed :(");
    }

    addr as *mut u8
}

fn mmap_anon(size: usize, use_hugepages: bool) -> *mut u8 {
    let mut flags = libc::MAP_ANONYMOUS | libc::MAP_PRIVATE | libc::MAP_NORESERVE;
    if use_hugepages {
        flags |= libc::MAP_HUGETLB;
    }
    mmap(size, flags, -1, 0)
}

// Returns the time required to touch all the pages.
fn touch_pages(addr: *mut u8, size: usize, page_size: usize, randomize_page_walk: bool) -> u128 {
    // This is safe as long as we pass a valid addr and len.
    let slice = unsafe { slice::from_raw_parts_mut(addr, size) };

    // Stagger and shuffle the indices of the pages we're going to touch.
    let mut v: Vec<usize> = (0..slice.len()).step_by(page_size).collect();

    if randomize_page_walk {
        v.as_mut_slice().shuffle(&mut thread_rng());
    }

    let t1 = Instant::now();

    for i in v {
        // Touch the page.
        slice[i] = 1;
        inc(&NUM_WRITES);
    }

    // Return the elapsed duration in microseconds.
    Instant::now().duration_since(t1).as_micros()
}

fn main() {
    let cmd_arguments = App::new("uffd")
        .version(crate_version!())
        .author(crate_authors!())
        .about("Play around with uffd.")
        .arg(
            Arg::with_name("randomize-page-walk")
                .long("randomize-page-walk")
                .help(
                    "Touch the memory pages in a random order (as opposed to doing it \
                     sequentially)",
                )
                .required(false),
        )
        .arg(
            Arg::with_name("size")
                .long("size")
                .help("The size (in bytes) of the memory area.")
                .takes_value(true)
                .required(true),
        )
        .arg(
            Arg::with_name("page-size")
                .long("page-size")
                .help(
                    "Page size in bytes. Will use MAP_HUGETLB for anonymous mappings when \
                     this is greater than 4KiB.",
                )
                .takes_value(true)
                .default_value("4096")
                .possible_values(&["4096", "2097152"]),
        )
        .arg(
            Arg::with_name("pseudo-page-size")
                .long("pseudo-page-size")
                .help("Pseudo page size.")
                .takes_value(true)
                .required(true),
        )
        .arg(
            Arg::with_name("uffd-anon")
                .long("uffd-anon")
                .help(
                    "Resolve faults via an uffd which copies data from a mmap-ed anonymous \
                     memory region.",
                )
                .takes_value(false)
                .required(false),
        )
        .arg(
            Arg::with_name("uffd-file")
                .long("uffd-file")
                .help(
                    "Resolve faults via an uffd which copies data from a memory mapped \
                     file, starting at offset 0. The length of the file should be greater or equal \
                     than the value specified for --size.",
                )
                .takes_value(true)
                .required(false)
                .conflicts_with("uffd-anon"),
        )
        .arg(
            Arg::with_name("uffd-simple")
                .long("uffd-simple")
                .help(
                    "Resolve faults via an uffd which copies data from a pre-allocated buffer \
                     equal to pseudo-page-size in length.",
                )
                .required(false)
                .conflicts_with_all(&["uffd-anon", "uffd-file"]),
        )
        .arg(
            Arg::with_name("uffd-zeropage")
                .long("uffd-zeropage")
                .help(
                    "Resolve faults via an uffd which initializes all pages to the zero page. \
                     Does not work with hugepages.",
                )
                .required(false)
                .conflicts_with_all(&["uffd-anon", "uffd-file", "uffd-simple", "hugepages"]),
        )
        .get_matches();

    let randomize_page_walk = cmd_arguments.is_present("randomize-page-walk");

    // The first unwrap is ok because the argument is required.
    let size = cmd_arguments
        .value_of("size")
        .unwrap()
        .parse::<usize>()
        .expect("Error parsing the size of the memory area");

    // The first unwrap is ok because the argument has a default value.
    let page_size = cmd_arguments
        .value_of("page-size")
        .unwrap()
        .parse::<usize>()
        .expect("Error parsing value of page-size");

    let use_hugepages = page_size > 4096;

    // The first unwrap is ok because the argument is required.
    let pseudo_page_size = cmd_arguments
        .value_of("pseudo-page-size")
        .unwrap()
        .parse::<usize>()
        .expect("Error parsing value of pseudo-page-size");

    if size < pseudo_page_size || size % pseudo_page_size != 0 {
        exit_msg(
            format!(
                "size({}) must be a non-zero multiple of pseudo-page-size({})",
                size, pseudo_page_size
            )
            .as_str(),
        );
    }

    if pseudo_page_size < page_size || pseudo_page_size % page_size != 0 {
        exit_msg(
            format!(
                "pseudo-page-size({}) must be a non-zero multiple of page-size({})",
                pseudo_page_size, page_size
            )
            .as_str(),
        );
    }

    // Allocate a private anonymous mmap-ed region for the main memory area.
    let addr = mmap_anon(size, use_hugepages);

    // Start an uffd thread to handle faults for the main memory area if the user specified
    // an appropriate cmdline parameter.
    if cmd_arguments.is_present("uffd-anon") {
        let uffd_addr = mmap_anon(size, use_hugepages);

        // Safe because the addresses and length are valid.
        let mut uffd = unsafe {
            MmapUffd::with_regions(
                &[(addr as u64, uffd_addr as u64, size as u64)],
                pseudo_page_size as u64,
            )
        }
        .expect("Cannot create MmapUffd object.");

        thread::spawn(move || loop {
            if uffd.handle_next().is_err() {
                exit_msg("uffd.handle_next error");
            }
            inc(&NUM_FAULTS);
        });
    } else if cmd_arguments.is_present("uffd-file") {
        // The unwrap cannot file because the argument is present.
        let file = File::open(cmd_arguments.value_of("uffd-file").unwrap())
            .expect("Cannot open the path specified as the value of --uffd-file");

        let metadata = file.metadata().expect("Cannot retrieve uffd-file metadata");
        if metadata.len() < size as u64 {
            exit_msg("The length of the file specified as the value of --uffd-file is shorter than --size");
        }

        let mut uffd = unsafe {
            MmapUffd::with_ranges(
                &[Range::new(addr as u64, Rc::new(file), 0, size)],
                pseudo_page_size as u64,
            )
        }
        .expect("Cannot create MmapUffd object.");

        thread::spawn(move || loop {
            if uffd.handle_next().is_err() {
                exit_msg("uffd.handle_next error");
            }
            inc(&NUM_FAULTS);
        });
    } else if cmd_arguments.is_present("uffd-simple") {
        let mut uffd = unsafe {
            SimpleUffd::with_regions(&[(addr as u64, size as u64)], pseudo_page_size, false)
        }
        .expect("Cannot create SimpleUffd object.");

        thread::spawn(move || loop {
            if uffd.handle_next().is_err() {
                exit_msg("uffd.handle_next error");
            }
            inc(&NUM_FAULTS);
        });
    } else if cmd_arguments.is_present("uffd-zeropage") {
        let mut uffd = unsafe {
            SimpleUffd::with_regions(&[(addr as u64, size as u64)], pseudo_page_size, true)
        }
        .expect("Cannot create SimpleUffd object.");

        thread::spawn(move || loop {
            if uffd.handle_next().is_err() {
                exit_msg("uffd.handle_next error");
            }
            inc(&NUM_FAULTS);
        });
    }

    let delta = touch_pages(addr, size, page_size, randomize_page_walk);

    println!("{} {}", delta, get(&NUM_FAULTS));
}
