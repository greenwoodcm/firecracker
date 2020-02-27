// Copyright 2020 Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0
extern crate bincode;
extern crate serde;
extern crate serde_derive;
extern crate serde_json;
extern crate snapshot_derive;
extern crate kvm_bindings;

pub mod primitives;
pub mod version_map;

use primitives::*;
use serde_derive::{Deserialize, Serialize};
use snapshot_derive::Versionize;
use std::collections::hash_map::HashMap;
use std::io::{Read, Write};
use version_map::VersionMap;

// 256k max section size.
const SNAPSHOT_MAX_SECTION_SIZE: usize = 0x40000;
const SNAPSHOT_FORMAT_VERSION: u16 = 1;
const BASE_MAGIC_ID_MASK: u64 = !0xFFFFu64;

#[cfg(target_arch = "x86_64")]
const BASE_MAGIC_ID: u64 = 0x0710_1984_8664_0000u64;

#[cfg(target_arch = "aarch64")]
const BASE_MAGIC_ID: u64 = 0x0710_1984_AAAA_0000u64;

// Returns format version if arch id is valid.
// Returns none otherwise.
fn validate_magic_id(magic_id: u64) -> Option<u16> {
    let magic_arch = magic_id & BASE_MAGIC_ID_MASK;
    if magic_arch == BASE_MAGIC_ID {
        return Some((magic_id & !BASE_MAGIC_ID_MASK) as u16);
    }
    None
}

fn build_magic_id(format_version: u16) -> u64 {
    BASE_MAGIC_ID | format_version as u64
}

/// Firecracker snapshot format.
///  
///  |----------------------------|
///  |         SnapshotHdr        |
///  |----------------------------|
///  |         Section  #1        |
///  |----------------------------|
///  |         Section  #2        |
///  |----------------------------|
///  |         Section  #3        |
///  |----------------------------|
///             ..........

#[derive(Default, Debug, Versionize)]
struct SnapshotHdr {
    /// Snapshot data version (firecracker version).
    data_version: u16,
    /// Number of sections
    section_count: u16,
}

pub struct Snapshot {
    hdr: SnapshotHdr,
    format_version: u16,
    version_map: VersionMap,
    sections: HashMap<String, Section>,
    // Required for serialization.
    target_version: u16,
}

#[derive(Default, Debug, Versionize)]
pub struct Section {
    name: String,
    data: Vec<u8>,
}

/// Trait that provides an implementation to deconstruct/restore structs
/// into typed fields backed by the Snapshot storage.
/// This trait is automatically implemented on user specified structs
/// or otherwise manually implemented.
pub trait Versionize {
    fn serialize<W: Write>(
        &self,
        writer: &mut W,
        version_map: &VersionMap,
        target_app_version: u16,
    );
    fn deserialize<R: Read>(reader: &mut R, version_map: &VersionMap, src_app_version: u16)
        -> Self;

    fn name() -> String;
    // Returns latest struct version.
    fn version() -> u16;
}

impl Snapshot {
    pub fn new(version_map: VersionMap, target_version: u16) -> std::io::Result<Snapshot> {
        Ok(Snapshot {
            version_map,
            hdr: SnapshotHdr::default(),
            format_version: SNAPSHOT_FORMAT_VERSION,
            sections: HashMap::new(),
            target_version,
        })
    }

    pub fn load<T>(mut reader: &mut T, version_map: VersionMap) -> std::io::Result<Snapshot>
    where
        T: Read,
    {
        let format_version_map = Self::format_version_map();
        let magic_id =
            <u64 as Versionize>::deserialize(&mut reader, &format_version_map, 0 /* unused */);
        let format_version = validate_magic_id(magic_id).unwrap();
        let hdr: SnapshotHdr =
            SnapshotHdr::deserialize(&mut reader, &format_version_map, format_version);
        let mut sections = HashMap::new();

        for _ in 0..hdr.section_count {
            let section = Section::deserialize(&mut reader, &format_version_map, format_version);
            sections.insert(section.name.clone(), section);
        }

        Ok(Snapshot {
            version_map,
            hdr,
            format_version,
            sections,
            // Not used when loading a snapshot.
            target_version: 0,
        })
    }

    pub fn save<T>(&mut self, mut writer: &mut T) -> std::io::Result<()>
    where
        T: std::io::Write,
    {
        self.hdr = SnapshotHdr {
            data_version: self.target_version,
            section_count: self.sections.len() as u16,
        };

        let format_version_map = Self::format_version_map();
        let magic_id = build_magic_id(format_version_map.get_latest_version());

        // Serialize magic id using the format version map.
        magic_id.serialize(&mut writer, &format_version_map, 0 /* unused */);
        // Serialize header using the format version map.
        self.hdr.serialize(
            &mut writer,
            &format_version_map,
            format_version_map.get_latest_version(),
        );

        // Serialize all the sections.
        for (_, section) in &self.sections {
            // The sections are already serialized.
            section.serialize(
                &mut writer,
                &format_version_map,
                format_version_map.get_latest_version(),
            );
        }
        writer.flush()?;

        Ok(())
    }

    fn read_section<T>(&mut self, name: &str) -> std::io::Result<Option<T>>
    where
        T: Versionize + 'static,
    {
        if self.sections.contains_key(name) {
            let section = &mut self.sections.get_mut(name).unwrap();
            return Ok(Some(T::deserialize(
                &mut section.data.as_mut_slice().as_ref(),
                &self.version_map,
                self.hdr.data_version,
            )));
        }
        Ok(None)
    }

    fn write_section<T>(&mut self, name: &str, object: &T) -> std::io::Result<()>
    where
        T: Versionize + 'static,
    {
        let mut new_section = Section {
            name: name.to_owned(),
            data: vec![0; SNAPSHOT_MAX_SECTION_SIZE],
        };

        let slice = &mut new_section.data.as_mut_slice();
        object.serialize(slice, &self.version_map, self.target_version);
        // Resize vec to serialized section len.
        let serialized_len =
            slice.as_ptr() as usize - new_section.data.as_slice().as_ptr() as usize;
        new_section.data.truncate(serialized_len);
        self.sections.insert(name.to_owned(), new_section);
        Ok(())
    }

    fn format_version_map() -> VersionMap {
        // Firecracker snapshot format version 1.
        VersionMap::new()
    }
}

#[inline]
pub fn bench_restore_v1() {
    let mut snapshot_mem = std::fs::File::open("/tmp/snapshot.fcs").unwrap();
    let vm = VersionMap::new();

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
    }

    let mut loaded_snapshot = Snapshot::load(&mut snapshot_mem, vm.clone()).unwrap();

    for _ in 0..100 {
        if let Some(mut state) = loaded_snapshot
            .read_section::<MmioDeviceState>("first")
            .unwrap()
        {
            //println!("Restore state1 {:?}", state1);
            state.irq = 0;
        }
        if let Some(mut state) = loaded_snapshot
            .read_section::<MmioDeviceState>("second")
            .unwrap()
        {
            //println!("Restore state2 {:?}", state2);
            state.irq = 0;
        }
    }
}

mod tests {
    #![allow(non_upper_case_globals)]
    #![allow(non_camel_case_types)]
    #![allow(non_snake_case)]
    use super::*;
    use std::io::{Seek, SeekFrom};
    pub type __s8 = ::std::os::raw::c_schar;
    pub type __u8 = ::std::os::raw::c_uchar;
    pub type __s16 = ::std::os::raw::c_short;
    pub type __u16 = ::std::os::raw::c_ushort;
    pub type __s32 = ::std::os::raw::c_int;
    pub type __u32 = ::std::os::raw::c_uint;
    pub type __s64 = ::std::os::raw::c_longlong;
    pub type __u64 = ::std::os::raw::c_ulonglong;

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
            2 => TestState::Two,
            _ => TestState::Two,
        }
    }
    fn test_state_default_two(_input: &TestState, target_version: u16) -> TestState {
        println!("test_state_default_two target version {}", target_version);
        match target_version {
            3 => TestState::Three,
            2 => TestState::Two,
            _ => TestState::One,
        }
    }

    #[repr(C)]
    #[derive(Copy, Debug, Clone, Versionize, PartialEq)]
    pub struct kvm_lapic_state {
        pub regs: [::std::os::raw::c_char; 32],
    }

    #[derive(Default, Copy, Debug, Clone, Versionize, PartialEq)]
    pub struct ArrayElement {
        x: u8,
        y: u8,
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
            default_fn = "default_error",
            semantic_ser_fn = "serialize_error_semantic",
            semantic_de_fn = "deserialize_error_semantic"
        )]
        pub error: String,
        #[snapshot(default = 128, start_version = 4)]
        arr: [ArrayElement; 2],
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
        "default_error".to_owned()
    }

    #[test]
    fn test_struct_default_fn() {
        #[derive(Versionize, Clone, Default)]
        struct Test1 {
            field1: u32,
        };

        #[derive(Versionize, Clone, Default)]
        struct Test {
            field1: u32,
            #[snapshot(start_version=2, default_fn="field2_default")]
            field2: u64,
            #[snapshot(start_version=3, default_fn="field3_default")]
            field3: String,
            #[snapshot(start_version=4, default_fn="field4_default")]
            field4: Vec<u64>
        }
        fn field2_default(_: u16) -> u64 {
            20
        }
        fn field3_default(_: u16) -> String {
            "default".to_owned()
        }
        fn field4_default(_: u16) -> Vec<u64> {
            vec![1,2,3,4]
        }

        let mut vm = VersionMap::new();
        vm.new_version()
        .set_type_version(Test::name(), 2)
        .new_version()
        .set_type_version(Test::name(), 3)
        .new_version()
        .set_type_version(Test::name(), 4);
        let state = Test {
            field1: 1,
            field2: 2,
            field3: "test".to_owned(),
            field4: vec![4,3,2,1],
        };

        let state_1 = Test1 {
            field1: 1,
        };

        let mut snapshot_mem = vec![0u8; 1024];
        
        // Serialize as v1.
        let mut snapshot = Snapshot::new(vm.clone(), 1).unwrap();
        snapshot.write_section("test", &state_1).unwrap();
        snapshot.save(&mut snapshot_mem.as_mut_slice()).unwrap();

        snapshot = Snapshot::load(&mut snapshot_mem.as_slice(), vm.clone()).unwrap();
        let restored_state: Test= snapshot.read_section::<Test>("test").unwrap().unwrap();
        assert_eq!(restored_state.field1, state_1.field1);
        assert_eq!(restored_state.field2, 20);
        assert_eq!(restored_state.field3, "default");
        assert_eq!(restored_state.field4, vec![1,2,3,4]);

        // Serialize as v2.
        let mut snapshot = Snapshot::new(vm.clone(), 2).unwrap();
        snapshot.write_section("test", &state).unwrap();
        snapshot.save(&mut snapshot_mem.as_mut_slice()).unwrap();

        snapshot = Snapshot::load(&mut snapshot_mem.as_slice(), vm.clone()).unwrap();
        let restored_state: Test= snapshot.read_section::<Test>("test").unwrap().unwrap();
        assert_eq!(restored_state.field1, state.field1);
        assert_eq!(restored_state.field2, 2);
        assert_eq!(restored_state.field3, "default");
        assert_eq!(restored_state.field4, vec![1,2,3,4]);
        
        // Serialize as v3.
        let mut snapshot = Snapshot::new(vm.clone(), 3).unwrap();
        snapshot.write_section("test", &state).unwrap();
        snapshot.save(&mut snapshot_mem.as_mut_slice()).unwrap();

        snapshot = Snapshot::load(&mut snapshot_mem.as_slice(), vm.clone()).unwrap();
        let restored_state: Test= snapshot.read_section::<Test>("test").unwrap().unwrap();
        assert_eq!(restored_state.field1, state.field1);
        assert_eq!(restored_state.field2, 2);
        assert_eq!(restored_state.field3, "test");
        assert_eq!(restored_state.field4, vec![1,2,3,4]);

         // Serialize as v4.
         let mut snapshot = Snapshot::new(vm.clone(), 4).unwrap();
         snapshot.write_section("test", &state).unwrap();
         snapshot.save(&mut snapshot_mem.as_mut_slice()).unwrap();
 
         snapshot = Snapshot::load(&mut snapshot_mem.as_slice(), vm.clone()).unwrap();
         let restored_state: Test= snapshot.read_section::<Test>("test").unwrap().unwrap();
         assert_eq!(restored_state.field1, state.field1);
         assert_eq!(restored_state.field2, 2);
         assert_eq!(restored_state.field3, "test");
         assert_eq!(restored_state.field4, vec![4,3,2,1]);
    }

    #[test]
    fn test_union_version() {
        #[repr(C)]
        #[derive(Versionize, Copy, Clone)]
        union kvm_irq_level__bindgen_ty_1 {
            pub irq: __u32,
            pub status: __s32,

            #[snapshot(start_version=1, end_version=1)]
            _bindgen_union_align: u32,

            #[snapshot(start_version=2)]
            pub extended_status: __s64,

            #[snapshot(start_version=2)]
            _bindgen_union_align_2: [u64; 4usize],
        }

        impl Default for kvm_irq_level__bindgen_ty_1 {
            fn default() -> Self {
                unsafe { ::std::mem::zeroed() }
            }
        }

        let mut state = kvm_irq_level__bindgen_ty_1::default();
        unsafe { 
            state.extended_status = 0x1234_5678_8765_4321;
        }

        let vm = VersionMap::new();
        let mut snapshot_mem = vec![0u8; 1024 * 2];
        // Serialize as v1.
        let mut snapshot = Snapshot::new(vm.clone(), 1).unwrap();
        snapshot.write_section("test", &state).unwrap();
        snapshot.save(&mut snapshot_mem.as_mut_slice()).unwrap();

        snapshot = Snapshot::load(&mut snapshot_mem.as_slice(), vm.clone()).unwrap();
        let restored_state = snapshot.read_section::<kvm_irq_level__bindgen_ty_1>("test").unwrap().unwrap();
        unsafe { 
            assert_eq!(restored_state.irq, 0x8765_4321);
        }
    }
    #[test]
    fn test_kvm_bindings_struct() {
        #[repr(C)]
        #[derive(Versionize, Debug, Default, Copy, Clone, PartialEq)]
        pub struct kvm_pit_config {
            pub flags: __u32,
            pub pad: [__u32; 15usize],
        }

        let state = kvm_pit_config {
            flags: 123456,
            pad: [0; 15usize]
        };

        let vm = VersionMap::new();
        let mut snapshot_mem = vec![0u8; 1024 * 2];
        // Serialize as v1.
        let mut snapshot = Snapshot::new(vm.clone(), 1).unwrap();
        snapshot.write_section("test", &state).unwrap();
        snapshot.save(&mut snapshot_mem.as_mut_slice()).unwrap();

        snapshot = Snapshot::load(&mut snapshot_mem.as_slice(), vm.clone()).unwrap();
        let restored_state = snapshot.read_section::<kvm_pit_config>("test").unwrap().unwrap();
        println!("State: {:?}", restored_state);
        // Check if we serialized x correctly, that is if semantic_x() was called.
        assert_eq!(restored_state, state);
    }

    #[test]
    fn test_basic_add_remove_field() {
        #[derive(Versionize, Debug, PartialEq, Clone)]
        pub struct A {
            #[snapshot(start_version = 1, end_version = 1)]
            x: u32,
            y: String,
            #[snapshot(start_version = 2, default_fn = "default_A_z")]
            z: String,
            #[snapshot(
                start_version = 3,
                semantic_ser_fn="semantic_x"
            )]
            q: u64,
        }

        #[derive(Versionize, Debug, PartialEq, Clone)]
        pub struct B {
            a: A,
            b: u64,
        }

        fn default_A_z(source_version: u16) -> String {
            "whatever".to_owned()
        }
     
        fn semantic_x(input: &mut A, target_version: u16) {
            input.x = input.q as u32;
        }

        let mut vm = VersionMap::new();
        vm.new_version()
            .set_type_version(A::name(), 2)
            .new_version()
            .set_type_version(A::name(), 3);

        let state = B {
            a: A {
                x: 0,
                y: "test".to_owned(),
                z: "basic".to_owned(),
                q: 1234,
            },
            b: 20,
        };

        let mut snapshot_mem = vec![0u8; 1024 * 2];

        // Serialize as v1.
        let mut snapshot = Snapshot::new(vm.clone(), 1).unwrap();
        snapshot.write_section("test", &state).unwrap();
        snapshot.save(&mut snapshot_mem.as_mut_slice()).unwrap();

        snapshot = Snapshot::load(&mut snapshot_mem.as_slice(), vm.clone()).unwrap();
        let mut restored_state = snapshot.read_section::<B>("test").unwrap().unwrap();
        println!("State: {:?}", restored_state);
        // Check if we serialized x correctly, that is if semantic_x() was called.
        assert_eq!(restored_state.a.x, 1234);
        assert_eq!(restored_state.a.z, stringify!(whatever));

        // Serialize as v2.
        snapshot = Snapshot::new(vm.clone(), 2).unwrap();
        snapshot.write_section("test", &state).unwrap();
        snapshot.save(&mut snapshot_mem.as_mut_slice()).unwrap();

        snapshot = Snapshot::load(&mut snapshot_mem.as_slice(), vm.clone()).unwrap();
        restored_state = snapshot.read_section::<B>("test").unwrap().unwrap();
        println!("State: {:?}", restored_state);
        // Check if x was not serialized, it should be 0.
        assert_eq!(restored_state.a.x, 0);
        // z field was added in version to, make sure it contains the original value
        assert_eq!(restored_state.a.z, stringify!(basic));

        // Serialize as v3.
        snapshot = Snapshot::new(vm.clone(), 3).unwrap();
        snapshot.write_section("test", &state).unwrap();
        snapshot.save(&mut snapshot_mem.as_mut_slice()).unwrap();

        snapshot = Snapshot::load(&mut snapshot_mem.as_slice(), vm.clone()).unwrap();
        restored_state = snapshot.read_section::<B>("test").unwrap().unwrap();
        println!("State: {:?}", restored_state);
        // Check if x was not serialized, it should be 0.
        assert_eq!(restored_state.a.x, 0);
        // z field was added in version to, make sure it contains the original value
        assert_eq!(restored_state.a.z, stringify!(basic));
        assert_eq!(restored_state.a.q, 1234);
    }

    #[test]
    fn test_serialize_older_2_versions() {
        // App v1 starts here. All structs/enums are at v1.
        let mut vm = VersionMap::new();
        // App v2 starts here,
        vm.new_version()
            .set_type_version(MmioDeviceState::name(), 2)
            .set_type_version(TestState::name(), 2)
            // App v3 starts here,
            .new_version()
            .set_type_version(MmioDeviceState::name(), 3)
            .set_type_version(TestState::name(), 3)
            // App v4 starts here,
            .new_version()
            .set_type_version(MmioDeviceState::name(), 4);

        let mut snapshot_mem = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open("/tmp/snapshot.fcs")
            .unwrap();

        let target_app_version = 1;
        let mut snapshot = Snapshot::new(vm.clone(), target_app_version).unwrap();

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
            error: "alabalaportocala".to_owned(),
            test: TestState::Three,
            arr: [ArrayElement { x: 1, y: 5 }; 2],
        };

        state.lapics.push(lapic.clone());
        state.lapics.push(lapic.clone());
        state.lapics.push(lapic.clone());

        snapshot.write_section("first", &state).unwrap();
        let mut state2 = state.clone();
        state2.addr = 5678;
        state2.test = TestState::One;

        snapshot.write_section("second", &state2).unwrap();
        snapshot.write_section("lapic", &lapic).unwrap();

        let _ = snapshot.save(&mut snapshot_mem);

        println!("Saved");

        snapshot_mem.seek(SeekFrom::Start(0)).unwrap();

        let mut loaded_snapshot = Snapshot::load(&mut snapshot_mem, vm.clone()).unwrap();
        let state1: MmioDeviceState = loaded_snapshot
            .read_section::<MmioDeviceState>("first")
            .unwrap()
            .unwrap();
        println!("Restore state1 {:?}", state1);
        assert_eq!(state1.addr, 1234);
        assert_eq!(state1.irq, 1337);

        let state2 = loaded_snapshot
            .read_section::<MmioDeviceState>("second")
            .unwrap()
            .unwrap();
        println!("Restore state2 {:?}", state2);
        assert_eq!(state2.addr, 5678);
        assert_eq!(state2.irq, 1337);
    }

    #[test]
    fn test_rollback_2_versions() {
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
        }

        let mut snapshot_file = std::fs::File::open("/tmp/snapshot.fcs").unwrap();
        let vm = VersionMap::new();
        let mut snapshot = Snapshot::load(&mut snapshot_file, vm.clone()).unwrap();

        let state1: MmioDeviceState = snapshot
            .read_section::<MmioDeviceState>("first")
            .unwrap()
            .unwrap();
        assert_eq!(state1.addr, 1234);
        assert_eq!(state1.irq, 1337);

        let state2 = snapshot
            .read_section::<MmioDeviceState>("second")
            .unwrap()
            .unwrap();
        assert_eq!(state2.addr, 5678);
        assert_eq!(state2.irq, 1337);
    }

    #[test]
    fn test_live_update_2_versions() {
        let mut vm = VersionMap::new();
        vm.new_version()
            .set_type_version(MmioDeviceState::name(), 2)
            .set_type_version(TestState::name(), 2)
            .new_version()
            .set_type_version(MmioDeviceState::name(), 3)
            .set_type_version(TestState::name(), 3)
            .new_version()
            .set_type_version(MmioDeviceState::name(), 4);

        let mut snapshot_mem = std::fs::OpenOptions::new()
            .read(true)
            .open("/tmp/snapshot.fcs")
            .unwrap();

        let mut loaded_snapshot = Snapshot::load(&mut snapshot_mem, vm.clone()).unwrap();
        let state1: MmioDeviceState = loaded_snapshot
            .read_section::<MmioDeviceState>("first")
            .unwrap()
            .unwrap();
        println!("Restore state1 {:?}", state1);
        assert_eq!(state1.error, "alabalaportocala");
        assert_eq!(state1.test, TestState::One);

        let state2 = loaded_snapshot
            .read_section::<MmioDeviceState>("second")
            .unwrap()
            .unwrap();
        println!("Restore state2 {:?}", state2);
        assert_eq!(state2.error, "alabalaportocala");
    }
}
