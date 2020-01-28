// Copyright 2020 Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0
extern crate serde;
extern crate serde_derive;
extern crate snapshot_derive;
extern crate bincode;
extern crate serde_json;

use std::hash::{Hash, Hasher};
use std::collections::hash_map::DefaultHasher;

use serde::de::DeserializeOwned;
use serde::ser::Serialize;
use serde_derive::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;
include!("structs.rs");

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
pub trait Snapshotable {
    fn snapshot(&self, version: u16) -> Vec<u8>;
    fn restore<R: std::io::Read>(mut reader: &mut R) -> Self;
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

    pub fn save_to_mem(&mut self, app_version: u16, description: String) -> std::io::Result<Vec<u8>> {
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

    /// Restore an object with specified id and type.
    pub fn deserialize<D>(&mut self) -> D
    where
        D: Snapshotable + 'static,
    {
        let mut slice = self.data.as_slice();
        D::restore(&mut slice)
    }

    /// Store an object with specified id and type.
    pub fn serialize<S>(&mut self, version: u16, object: &S)
    where
        S: Snapshotable + 'static,
    {
        self.data = object.snapshot(version);
    }
}

include!("/tmp/translator.rs");

#[inline]
pub fn bench_1mil_save_restore() {
    let mut snapshot = Snapshot::load(Path::new("/tmp/snap.fcs")).unwrap();
    
    for i in 0..10000 {
        let x: MmioDeviceState = snapshot.deserialize();
    }

}

mod tests {
    use super::*;
    
    #[test]
    fn test_save() {
        let mut snapshot = Snapshot::new(Path::new("/tmp/snap.fcs")).unwrap();

        let state = MmioDeviceState {
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
            flag: 90,
        };
        snapshot.serialize(2, &state);
        snapshot.save(1, "Testing".to_owned()).unwrap();

        // let state = MmioDeviceState {
        //     addr: 1234,
        //     irq: 3,
        //     device_activated: true,
        //     features_select: 123456,
        //     acked_features_select: 653421,
        //     queue_select: 2,
        //     interrupt_status: 88,
        //     driver_status: 0,
        //     config_generation: 0,
        //     queues: vec![0; 64],
        //     flag: 90,
        // };

        snapshot = Snapshot::load(Path::new("/tmp/snap.fcs")).unwrap();

        let x: MmioDeviceState = snapshot.deserialize();
        // let y: MmioDeviceState_v1 = snapshot.restore_object("test_object0".to_owned());

        println!("Restore as {:?}", x);
        // println!("Restore as {:?}", y);

        println!("ToJson = {}", serde_json::to_string(&x).unwrap());

        assert!(false);
    }
    // fn test_save() {
    //     let mut snapshot = Snapshot::new(Path::new("/tmp/snap.fcs")).unwrap();

    //     for i in 1..1000 {
    //         let p = Test_V1 {
    //             field1: i,
    //             field2: format!("xxx{}", i).to_owned(),
    //             field3: vec![i as u8; 8],
    //         };
    //         snapshot.store_object(format!("test_object{}", i).to_owned(), 2, &p);

    //     }

    //     snapshot.save(1, "Testing".to_owned()).unwrap();
    //     println!("Saved {} ", snapshot.data_size);
    //     let snap = SnapshotObject {
    //         kind: SnapshotObjectType::Field,
    //         id: "xxx".to_owned(),
    //         version: 0,
    //         offset: 0,
    //         len: 0
    //     };
    //     println!("Object metadata size per field {} ",  bincode::serialized_size(&snap).unwrap());


    //     snapshot = Snapshot::load(Path::new("/tmp/snap.fcs")).unwrap();

    //     let x: Test_V3 = snapshot.restore_object("test_object0".to_owned());
    //     let y: Test_V2 = snapshot.restore_object("test_object1".to_owned());
    //     let z: Test_V1 = snapshot.restore_object("test_object2".to_owned());

    //     println!("Restore as {:?}", x);
    //     println!("Restore as {:?}", y);
    //     println!("Restore as {:?}", z);

    //     println!("ToJson = {}", serde_json::to_string(&z).unwrap());

    //     assert!(false);
    // }
}
