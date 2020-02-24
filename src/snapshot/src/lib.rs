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
use std::io::{Read, Write};
use std::collections::hash_map::HashMap;


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

fn build_magic_id(format_version: u16) -> u64{
    BASE_MAGIC_ID | format_version as u64
}

/// Firecracker snapshot format version 1.
///  
///  |----------------------------|
///  |         SnapshotHdr        |
///  |----------------------------|
///  |         Section hdr        |
///  |----------------------------|
///  |    Section Bincode blob    |
///  |----------------------------|
///  |         Section hdr        |
///  |----------------------------|
///  |    Section Bincode blob    |
///  |----------------------------|
///             ..........
/// The header contains snapshot format version, firecracker version
/// and a description string.
/// Each section contains a header and the bincode blob encodes
/// one object.

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
    sections: HashMap<String, Section>
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
    fn serialize<W: Write>(&self, writer: &mut W, version_map: &VersionMap, target_app_version: u16);
    fn deserialize<R: Read>(reader: &mut R, version_map: &VersionMap, src_app_version: u16) -> Self;

    // Returns struct names.
    fn name() -> String;
    fn version() -> u16;
}

impl Snapshot {
    pub fn new(version_map: VersionMap) -> std::io::Result<Snapshot> {
        Ok(Snapshot {
            version_map,
            hdr: SnapshotHdr::default(),
            format_version: SNAPSHOT_FORMAT_VERSION,
            sections: HashMap::new(),
        })
    }

    pub fn load<T>(mut reader: &mut T, version_map: VersionMap) -> std::io::Result<Snapshot> 
        where T: Read 
    {
        let format_version_map = Self::format_version_map();
        let magic_id = <u64 as Versionize>::deserialize(&mut reader, &format_version_map, 0 /* unused */);
        let format_version = validate_magic_id(magic_id).unwrap();
        let hdr: SnapshotHdr = SnapshotHdr::deserialize(&mut reader, &format_version_map, format_version);
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
        })
    }

    pub fn save<T>(&mut self, mut writer: &mut T, target_app_version: u16) -> std::io::Result<()> 
        where T: std::io::Write 
    {
        self.hdr = SnapshotHdr {
            data_version: target_app_version,
            section_count: self.sections.len() as u16
        };

        let format_version_map = Self::format_version_map();
        let magic_id = build_magic_id(format_version_map.get_latest_version());

        // Serialize magic id using the format version map.
        magic_id.serialize(&mut writer, &format_version_map, 0/* unused */);
        // Serialize header using the format version map.
        self.hdr.serialize(&mut writer, &format_version_map, format_version_map.get_latest_version());
        
        // Serialize all the sections.
        for (_, section) in &self.sections {
            // The sections are already serialized.
            section.serialize(&mut writer, &format_version_map, format_version_map.get_latest_version());
        }
        writer.flush()?;

        Ok(())
    }
    
    fn read_section<T>(&mut self, name: &str) -> std::io::Result<Option<T>> 
        where T: Versionize + 'static
    {
        if self.sections.contains_key(name) {
            let section = &mut self.sections.get_mut(name).unwrap();
            return Ok(Some(T::deserialize(&mut section.data.as_mut_slice().as_ref(), &self.version_map, self.hdr.data_version)))
        }
        Ok(None)
    }
 
    fn write_section<T>(&mut self, name: &str, target_app_version: u16, object: &T) -> std::io::Result<()> 
        where T: Versionize + 'static
    {
        let mut new_section = Section {
            name: name.to_owned(),
            data: vec![0; SNAPSHOT_MAX_SECTION_SIZE]
        };

        let slice = &mut new_section.data.as_mut_slice();
        object.serialize(slice, &self.version_map, target_app_version);
        // Resize vec to serialized section len.
        let serialized_len = slice.as_ptr() as usize - new_section.data.as_slice().as_ptr() as usize;
        new_section.data.truncate(serialized_len);
        self.sections.insert(name.to_owned(), new_section);
        Ok(())
    } 

    fn format_version_map() -> VersionMap {
        // Firecracker snapshot format version 1.
        VersionMap::new()
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
        if let Some(mut state) = loaded_snapshot.read_section::<MmioDeviceState>("first").unwrap() {
            //println!("Restore state1 {:?}", state1);
            state.irq = 0;
        }
        if let Some(mut state) = loaded_snapshot.read_section::<MmioDeviceState>("second").unwrap() {
            //println!("Restore state2 {:?}", state2);
            state.irq = 0;
        }    
    }

}

mod tests {
    use super::*;
    use std::io::{Seek, SeekFrom};

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

    #[derive(Default, Copy, Debug, Clone, Versionize, PartialEq)]
    pub struct ArrayElement {
        x: u8,
        y: u8
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

 
    // #[derive(Versionize, Default, Clone)]
    // pub struct TestUnion {
    //     union_field: kvm_nested_state__bindgen_ty_1,
    // }

    // #[derive(Versionize, Copy, Clone)]
    // pub union kvm_nested_state__bindgen_ty_1 {
    //     pub vmx: kvm_vmx_nested_state,
    //     pub pad: [u8; 120usize],
    //     _bindgen_union_align: [u64; 15usize],
    // }

    // impl Default for kvm_nested_state__bindgen_ty_1 {
    //     fn default() -> Self {
    //         unsafe { ::std::mem::zeroed() }
    //     }
    // }

    // #[repr(C)]
    // #[derive(Versionize, Default, Copy, Clone, PartialEq)]
    // pub struct kvm_vmx_nested_state {
    //     pub vmxon_pa: u64,
    //     pub vmcs_pa: u64,
    //     pub smm: kvm_vmx_nested_state__bindgen_ty_1,
    // }

    // #[repr(C)]
    // #[derive(Versionize, Debug, Default, Copy, Clone, PartialEq)]
    // pub struct kvm_vmx_nested_state__bindgen_ty_1 {
    //     pub flags: u16,
    // }

    // #[repr(C)]
    // #[derive(Default)]
    // pub struct __IncompleteArrayField<T>(::std::marker::PhantomData<T>, [T; 0]);
    // impl<T> __IncompleteArrayField<T> {
    //     #[inline]
    //     pub fn new() -> Self {
    //         __IncompleteArrayField(::std::marker::PhantomData, [])
    //     }
    //     #[inline]
    //     pub unsafe fn as_ptr(&self) -> *const T {
    //         ::std::mem::transmute(self)
    //     }
    //     #[inline]
    //     pub unsafe fn as_mut_ptr(&mut self) -> *mut T {
    //         ::std::mem::transmute(self)
    //     }
    //     #[inline]
    //     pub unsafe fn as_slice(&self, len: usize) -> &[T] {
    //         ::std::slice::from_raw_parts(self.as_ptr(), len)
    //     }
    //     #[inline]
    //     pub unsafe fn as_mut_slice(&mut self, len: usize) -> &mut [T] {
    //         ::std::slice::from_raw_parts_mut(self.as_mut_ptr(), len)
    //     }
    // }
    // impl<T> ::std::fmt::Debug for __IncompleteArrayField<T> {
    //     fn fmt(&self, fmt: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
    //         fmt.write_str("__IncompleteArrayField")
    //     }
    // }
    // impl<T> ::std::clone::Clone for __IncompleteArrayField<T> {
    //     #[inline]
    //     fn clone(&self) -> Self {
    //         Self::new()
    //     }
    // }


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
            .read(true).write(true) 
            .open("/tmp/snapshot.fcs").unwrap();
    
        let mut snapshot = Snapshot::new(vm.clone()).unwrap();

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
            arr: [ArrayElement{x: 1, y: 5}; 2]
        };

        state.lapics.push(lapic.clone());
        state.lapics.push(lapic.clone());
        state.lapics.push(lapic.clone());

        let target_app_version = 1;

        snapshot.write_section("first", target_app_version, &state).unwrap();
        let mut state2 = state.clone();
        state2.addr = 5678;
        state2.test = TestState::One;

        snapshot.write_section("second", target_app_version, &state2).unwrap();
        snapshot.write_section("lapic", target_app_version, &lapic).unwrap();

        let _ = snapshot.save(&mut snapshot_mem, target_app_version);

        println!("Saved");

        snapshot_mem.seek(SeekFrom::Start(0)).unwrap();
        
        let mut loaded_snapshot = Snapshot::load(&mut snapshot_mem, vm.clone()).unwrap();
        let state1: MmioDeviceState = loaded_snapshot.read_section::<MmioDeviceState>("first").unwrap().unwrap();
        println!("Restore state1 {:?}", state1);
        assert_eq!(state1.addr, 1234);
        assert_eq!(state1.irq, 1337);

        let state2 = loaded_snapshot.read_section::<MmioDeviceState>("second").unwrap().unwrap();
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
    
        let state1: MmioDeviceState = snapshot.read_section::<MmioDeviceState>("first").unwrap().unwrap();
        assert_eq!(state1.addr, 1234);
        assert_eq!(state1.irq, 1337);
        
        let state2 = snapshot.read_section::<MmioDeviceState>("second").unwrap().unwrap();
        assert_eq!(state2.addr, 5678);
        assert_eq!(state2.irq, 1337);
    }

    #[test]
    fn test_live_update_2_versions() {
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
            .open("/tmp/snapshot.fcs").unwrap();
    
        let mut loaded_snapshot = Snapshot::load(&mut snapshot_mem, vm.clone()).unwrap();
        let state1: MmioDeviceState = loaded_snapshot.read_section::<MmioDeviceState>("first").unwrap().unwrap();
        println!("Restore state1 {:?}", state1);
        assert_eq!(state1.error, "alabalaportocala");
        assert_eq!(state1.test, TestState::One);
        
        let state2 = loaded_snapshot.read_section::<MmioDeviceState>("second").unwrap().unwrap();
        println!("Restore state2 {:?}", state2);
        assert_eq!(state2.error, "alabalaportocala");

    }
}
