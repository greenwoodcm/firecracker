// Copyright 2020 Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0
extern crate bincode;
extern crate serde;
extern crate serde_derive;
extern crate serde_json;
extern crate snapshot_derive;

use serde_derive::{Deserialize, Serialize};
use snapshot_derive::Versionize;
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;
//include!("structs.rs");

const SNAPSHOT_FORMAT_VERSION: u16 = 1;

/// Firecracker snapshot format version 1.
///  
///  |----------------------------|
///  |         SnapshotHdr        |
///  |----------------------------|
///  |       Bincode blob         |
///  |----------------------------|
///
/// The header contains snapshot format version, firecracker version
/// and a description string.
/// The objects ared stored in a bincoden blob.

#[derive(Debug, Serialize, Deserialize)]
struct SnapshotHdr {
    /// Snapshot format version.
    version: u16,
    /// Snapshot data version (firecracker version).
    data_version: u16,
    /// Short description of snapshot.  
    description: String,
}

pub struct Snapshot {
    file: Option<File>,
    data: Vec<u8>,
}

/// Trait that provides an implementation to deconstruct/restore structs
/// into typed fields backed by the Snapshot storage.
/// This trait is automatically implemented on user specified structs
/// or otherwise manually implemented.
pub trait Versionize {
    fn serialize<W: std::io::Write>(&self, writer: &mut W, version: u16);
    fn deserialize<R: std::io::Read>(reader: &mut R, version: u16) -> Self;

    // Returns struct name as string.
    fn name() -> String;
    fn version() -> u16;
}

impl Snapshot {
    //// Public API
    pub fn new_in_memory() -> std::io::Result<Snapshot> {
        Ok(Snapshot {
            file: None,
            data: Vec::new(),
        })
    }
    pub fn new(path: &Path) -> std::io::Result<Snapshot> {
        let file = OpenOptions::new().create(true).write(true).open(path)?;

        Ok(Snapshot {
            file: Some(file),
            data: Vec::new(),
        })
    }

    pub fn save_to_mem(
        &mut self,
        app_version: u16,
        description: String,
    ) -> std::io::Result<Vec<u8>> {
        let hdr = SnapshotHdr {
            version: SNAPSHOT_FORMAT_VERSION,
            data_version: app_version,
            description,
        };

        let mut snapshot_data = bincode::serialize(&hdr).unwrap();
        snapshot_data.append(&mut self.data);

        Ok(snapshot_data)
    }

    pub fn save(&mut self, app_version: u16, description: String) -> std::io::Result<()> {
        self.data = self.save_to_mem(app_version, description).unwrap();

        let mut file = self.file.as_ref().unwrap();
        file.set_len(0)?;
        file.seek(SeekFrom::Start(0))?;

        file.write(&self.data)?;
        file.sync_all()?;

        Ok(())
    }

    pub fn load_from_mem(mem: &mut [u8]) -> std::io::Result<Snapshot> {
        let mut snapshot = Snapshot {
            file: None,
            data: Vec::new(),
        };

        // Load the snapshot header.
        let hdr: SnapshotHdr = bincode::deserialize(&mem).unwrap();
        let hdr_size = bincode::serialized_size(&hdr).unwrap() as usize;

        // Store the bincode blob.
        snapshot.data = mem[hdr_size..].to_vec();

        Ok(snapshot)
    }

    pub fn load(path: &Path) -> std::io::Result<Snapshot> {
        let mut file = OpenOptions::new().read(true).write(true).open(path)?;
        let mut slice = Vec::new();
        file.read_to_end(&mut slice)?;

        Self::load_from_mem(&mut slice)
    }

    pub fn deserialize<D>(&mut self) -> D
    where
        D: Versionize + 'static,
    {
        let mut slice = self.data.as_slice();
        D::deserialize(&mut slice, 1)
    }

    pub fn serialize<S>(&mut self, version: u16, object: &S)
    where
        S: Versionize + 'static,
    {
        // self.data.resize(1024*1024*2, 0);
        object.serialize(&mut self.data, version);
    }
}

macro_rules! primitive_versionize {
    ($ty:ident) => {
        impl Versionize for $ty {
            #[inline]
            fn serialize<W: std::io::Write>(&self, writer: &mut W, _version: u16) {
                bincode::serialize_into(writer, &self).unwrap();
            }
            #[inline]
            fn deserialize<R: std::io::Read>(mut reader: &mut R, _version: u16) -> Self {
                bincode::deserialize_from(&mut reader).unwrap()
            }

            // Not used.
            fn name() -> String {
                String::new()
            }
            // Not used.
            fn version() -> u16 {
                0
            }
        }
    };
}

primitive_versionize!(bool);
primitive_versionize!(isize);
primitive_versionize!(i8);
primitive_versionize!(i16);
primitive_versionize!(i32);
primitive_versionize!(i64);
primitive_versionize!(usize);
primitive_versionize!(u8);
primitive_versionize!(u16);
primitive_versionize!(u32);
primitive_versionize!(u64);
primitive_versionize!(f32);
primitive_versionize!(f64);
primitive_versionize!(char);
primitive_versionize!(String);
// primitive_versionize!(Option<T>);

// primitive_versionize!(str);

#[cfg(feature = "std")]
primitive_versionize!(CStr);
#[cfg(feature = "std")]
primitive_versionize!(CString);

impl<T> Versionize for Vec<T>
where
    T: Versionize,
{
    #[inline]
    fn serialize<W: std::io::Write>(&self, mut writer: &mut W, version: u16) {
        // Serialize in the same fashion as bincode:
        // len, T, T, ...
        bincode::serialize_into(&mut writer, &self.len()).unwrap();
        for obj in self {
            obj.serialize(writer, version);
        }
    }

    #[inline]
    fn deserialize<R: std::io::Read>(mut reader: &mut R, version: u16) -> Self {
        let mut v = Vec::new();
        let len: u64 = bincode::deserialize_from(&mut reader).unwrap();
        for _ in 0..len {
            let obj: T = T::deserialize(reader, version);
            v.push(obj);
        }
        v
    }

    // Not used.
    fn name() -> String {
        String::new()
    }
    // Not used.
    fn version() -> u16 {
        0
    }
}

// #[inline]
// pub fn bench_1mil_save_restore() {
//     let mut snapshot = Snapshot::load(Path::new("/tmp/snap.fcs")).unwrap();

//     for i in 0..10000 {
//         let x: MmioDeviceState = snapshot.deserialize();
//     }

// }

mod tests {
    use super::*;

    #[repr(u32)]
    #[derive(Debug, Versionize, Serialize, Deserialize, PartialEq)]
    pub enum TestState {
        One = 1,
        #[snapshot(start_version = 2, default_fn = "test_state_default_One")]
        Two = 2,
        #[snapshot(start_version = 3, default_fn = "test_state_default_One")]
        Three = 3,
    }

    impl Default for TestState {
        fn default() -> Self {
            Self::One
        }
    }

    fn test_state_default_One(input: &TestState, target_version: u16) -> TestState {
        match target_version {
            2 => {
                TestState::One
            }
            a => {
                TestState::One
            }
        }
    }

    #[repr(C)]
    #[derive(Copy, Debug, Clone, Versionize, PartialEq)]
    pub struct kvm_lapic_state {
        pub regs: [::std::os::raw::c_char; 32],
    }

    #[derive(Versionize, Debug, PartialEq)]
    pub struct MmioDeviceState {
        pub addr: u64,
        pub irq: u32,
        pub device_activated: bool,
        pub features_select: u32,
        pub acked_features_select: u32,
        pub queue_select: u32,
        pub interrupt_status: usize,
        pub driver_status: u32,
        pub config_generation: u32,
        pub queues: Vec<u8>,
        pub lapics: Vec<kvm_lapic_state>,
        #[snapshot(default = 128, start_version = 2)]
        pub flag: u8,
        #[snapshot(start_version = 3)]
        pub error: u64,
        #[snapshot(start_version = 3)]
        pub test: TestState,
    }

    #[test]
    fn test_save() {
        let mut snapshot = Snapshot::new(Path::new("/tmp/snap.fcs")).unwrap();

        let regs = [-5; 32usize];
        let lapic = kvm_lapic_state { regs };

        let mut state = MmioDeviceState {
            addr: 1234,
            irq: 3,
            device_activated: true,
            features_select: 123456,
            acked_features_select: 653421,
            queue_select: 2,
            interrupt_status: 88,
            driver_status: 0,
            config_generation: 0,
            queues: vec![0; 64],
            lapics: Vec::new(),
            flag: 90,
            error: 123,
            test: TestState::Two
        };

        state.lapics.push(lapic.clone());
        state.lapics.push(lapic.clone());
        state.lapics.push(lapic.clone());

        snapshot.serialize(1, &state);
        snapshot.save(1, "Testing".to_owned()).unwrap();

        println!("Saved");
        snapshot = Snapshot::load(Path::new("/tmp/snap.fcs")).unwrap();

        let x: MmioDeviceState = snapshot.deserialize();

        println!("Restore as {:?}", x);
        // println!("Restore as {:?}", y);

        // println!("ToJson = {}", serde_json::to_string(&x).unwrap());

        assert!(false);
    }
}
