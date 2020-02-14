// Copyright 2020 Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0
extern crate bincode;
extern crate serde;
extern crate serde_derive;
extern crate serde_json;
extern crate snapshot_derive;

pub mod version_map;

use version_map::VersionMap;
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

#[derive(Default, Debug, Serialize, Deserialize)]
struct SnapshotHdr {
    /// Snapshot format version.
    version: u16,
    /// Snapshot data version (firecracker version).
    data_version: u16,
    /// Short description of snapshot.  
    description: String,
}

pub struct Snapshot {
    hdr: SnapshotHdr,
    file: Option<File>,
    data: Vec<u8>,
    version_map: VersionMap,
}

/// Trait that provides an implementation to deconstruct/restore structs
/// into typed fields backed by the Snapshot storage.
/// This trait is automatically implemented on user specified structs
/// or otherwise manually implemented.
pub trait Versionize {
    fn serialize<W: std::io::Write>(&self, writer: &mut W, version_map: &VersionMap, app_version: u16);
    fn deserialize<R: std::io::Read>(reader: &mut R, version_map: &VersionMap, app_version: u16) -> Self;

    // Returns struct name as string.
    fn name() -> String;
    fn version() -> u16;
}

impl Snapshot {
    //// Public API
    pub fn new_in_memory(version_map: VersionMap) -> std::io::Result<Snapshot> {
        Ok(Snapshot {
            hdr: SnapshotHdr::default(),
            version_map,
            file: None,
            data: Vec::new(),
        })
    }
    pub fn new(path: &Path, version_map: VersionMap) -> std::io::Result<Snapshot> {
        let file = OpenOptions::new().create(true).write(true).open(path)?;

        Ok(Snapshot {
            hdr: SnapshotHdr::default(),
            version_map,
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

    pub fn load_from_mem(mem: &mut [u8], version_map: VersionMap) -> std::io::Result<Snapshot> {
        let mut snapshot = Snapshot {
            hdr: bincode::deserialize(&mem).unwrap(),
            version_map,
            file: None,
            data: Vec::new(),
        };

        // Load the snapshot header.
        let hdr_size = bincode::serialized_size(&snapshot.hdr).unwrap() as usize;

        // Store the bincode blob.
        snapshot.data = mem[hdr_size..].to_vec();

        Ok(snapshot)
    }

    pub fn load(path: &Path, version_map: VersionMap) -> std::io::Result<Snapshot> {
        let mut file = OpenOptions::new().read(true).write(true).open(path)?;
        let mut slice = Vec::new();
        file.read_to_end(&mut slice)?;

        Self::load_from_mem(&mut slice, version_map)
    }

    pub fn deserialize<D>(&mut self) -> D
    where
        D: Versionize + 'static,
    {
        let mut slice = self.data.as_slice();
        // We always deserialize into latest version from the app version specified in the header.
        D::deserialize(&mut slice, &self.version_map, self.hdr.data_version)
    }

    pub fn serialize<S>(&mut self, app_version: u16, object: &S)
    where
        S: Versionize + 'static,
    {
        object.serialize(&mut self.data, &self.version_map, app_version);
    }
}

macro_rules! primitive_versionize {
    ($ty:ident) => {
        impl Versionize for $ty {
            #[inline]
            fn serialize<W: std::io::Write>(&self, writer: &mut W, _version_map: &VersionMap, _version: u16) {
                bincode::serialize_into(writer, &self).unwrap();
            }
            #[inline]
            fn deserialize<R: std::io::Read>(mut reader: &mut R, _version_map: &VersionMap, _version: u16) -> Self {
                bincode::deserialize_from(&mut reader).unwrap()
            }

            // Not used.
            fn name() -> String {
                String::new()
            }
            // Not used.
            fn version() -> u16 {
                1
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
    fn serialize<W: std::io::Write>(&self, mut writer: &mut W, version_map: &VersionMap, app_version: u16) {
        // Serialize in the same fashion as bincode:
        // len, T, T, ...
        bincode::serialize_into(&mut writer, &self.len()).unwrap();
        for obj in self {
            obj.serialize(writer, version_map, app_version);
        }
    }

    #[inline]
    fn deserialize<R: std::io::Read>(mut reader: &mut R, version_map: &VersionMap, app_version: u16) -> Self {
        let mut v = Vec::new();
        let len: u64 = bincode::deserialize_from(&mut reader).unwrap();
        for _ in 0..len {
            let obj: T = T::deserialize(reader, version_map, app_version);
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
        1
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
    #[derive(Debug, Versionize, Serialize, Deserialize, PartialEq, Clone)]
    pub enum TestState {
        One = 1,
        #[snapshot(start_version = 2, default_fn = "test_state_default_one")]
        Two = 2,
        #[snapshot(start_version = 3, default_fn = "test_state_default_two")]
        Three = 3,
    }

    impl Default for TestState {
        fn default() -> Self {
            Self::One
        }
    }

    fn test_state_default_one(_input: &TestState, target_version: u16) -> TestState {
        println!("test_state_default_one target version {}", target_version);

        match target_version {
            2 => {
                TestState::Two
            }
            _ => {
                TestState::Two
            }
        }
    }
    fn test_state_default_two(_input: &TestState, target_version: u16) -> TestState {
        println!("test_state_default_two target version {}", target_version);
        match target_version {
            3 => {
                TestState::Three
            }
            2 => {
                TestState::Two
            }
            _ => {
                TestState::One
            }
        }
    }

    #[repr(C)]
    #[derive(Copy, Debug, Clone, Versionize, PartialEq)]
    pub struct kvm_lapic_state {
        pub regs: [::std::os::raw::c_char; 32],
    }

    #[derive(Versionize, Debug, PartialEq, Clone)]
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
        pub test: TestState,
        #[snapshot(default = 128, start_version = 2)]
        pub flag: u8,
        // Default_fn is called when deserializing from a version that does not
        // define this field.
        #[snapshot(
            start_version = 3, 
            default_fn="default_error", 
            semantic_ser_fn="serialize_error_semantic",
            semantic_de_fn="deserialize_error_semantic"
        )]
        pub error: String,
    }

    fn serialize_error_semantic(input: &mut MmioDeviceState, target_version: u16) {
        match target_version {
            1..=2 => {
                if input.error == "alabalaportocala" {
                    input.irq = 1337;
                } else {
                    input.irq = 1984;
                }
            }
            _ => {}
        }
    }

    fn deserialize_error_semantic(input: &mut MmioDeviceState, source_version: u16) {
        match source_version {
            1..=2 => {
                if input.irq == 1337 {
                    input.error = "alabalaportocala".to_owned();
                }
            }
            _ => {}
        }
    }
    fn default_error(_source_version: u16) -> String {
        "alabalaportocala".to_owned()
    }

    #[test]
    fn test_save() {
        // App v1 starts here. All structs/enums are at v1.
        let mut vm = VersionMap::new();
        // App v2 starts here,
        vm.new_version()
        .set_type_version(MmioDeviceState::name(), 2)
        .set_type_version(TestState::name(), 2)
        // App v3 starts here,
        .new_version()
        .set_type_version(MmioDeviceState::name(), 3)
        .set_type_version(TestState::name(), 3);


        let mut snapshot = Snapshot::new(Path::new("/tmp/snap.fcs"), vm.clone()).unwrap();

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
            error: "ceva".to_owned(),
            test: TestState::Three
        };

        state.lapics.push(lapic.clone());
        state.lapics.push(lapic.clone());
        state.lapics.push(lapic.clone());

        snapshot.serialize(2, &state);
        snapshot.save(2, "Testing".to_owned()).unwrap();

        println!("Saved");
        snapshot = Snapshot::load(Path::new("/tmp/snap.fcs"), vm).unwrap();

        let x: MmioDeviceState = snapshot.deserialize();

        println!("Restore as {:?}", x);
        // println!("Restore as {:?}", y);

        // println!("ToJson = {}", serde_json::to_string(&x).unwrap());

        assert!(false);
    }
}
