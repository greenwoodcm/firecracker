// Copyright 2020 Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0
extern crate serde;
extern crate serde_cbor;
extern crate serde_derive;
extern crate snapshot_derive;

use serde::de::DeserializeOwned;
use serde::ser::Serialize;
use serde_cbor::{from_slice, to_vec, Deserializer};
use serde_derive::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

const SNAPSHOT_FORMAT_VERSION: u16 = 1;

/// Firecracker snapshot format version 1.
///  
///  |----------------------------|
///  |         SnapshotHdr        |
///  |----------------------------|
///  |          Objects           |
///  |----------------------------|
///
/// The header contains snapshot format version, firecracker version
/// and a description string.
/// The objects ared stored as a vector of SnapshotObject entries which describe
/// the data. The SnapshotObject structure is followed by the cbor serialized
/// object.
///
/// The snapshot engine works as a data store, properties are created/read
/// using get/set_object.
///
/// Loading a snapshot does not trigger any version translation, it simply
/// loads all the objects into memory.

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SnapshotObjectType {
    Field,
    Struct,
    NestedStruct,
}

type SnapshotBlob = Vec<u8>;

#[derive(Debug, Serialize, Deserialize, Clone)]
/// Describes a snapshot property.
pub struct SnapshotObject {
    kind: SnapshotObjectType,
    // Object version
    version: u16,
    // Unique ID.
    id: String,
    // CBOR encoded data
    data: Vec<u8>,
}

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
    objects: HashMap<String, SnapshotObject>,
    file: Option<File>,
}

/// Trait that provides an implementation to deconstruct/restore structs
/// into typed fields backed by the Snapshot storage.
/// This trait is automatically implemented on user specified structs
/// or otherwise manually implemented.
pub trait Snapshotable {
    fn snapshot(&self, id: String, version: u16, snapshot: &mut Snapshot);
    fn restore(id: String, snapshot: &mut Snapshot) -> Self;
}

impl Snapshot {
    //// Public API 
    pub fn new_in_memory() -> std::io::Result<Snapshot> {
        Ok(Snapshot {
            objects: HashMap::new(),
            file: None,
        })
    }
    pub fn new(path: &Path) -> std::io::Result<Snapshot> {
        let file = OpenOptions::new().create(true).write(true).open(path)?;

        Ok(Snapshot {
            objects: HashMap::new(),
            file: Some(file),
        })
    }

    pub fn save_to_mem(&mut self, app_version: u16, description: String) -> std::io::Result<Vec<u8>> {
        // Serialize the header.
        let hdr = SnapshotHdr {
            version: SNAPSHOT_FORMAT_VERSION,
            data_version: app_version,
            description,
        };

        let mut snapshot_data = serde_cbor::to_vec(&hdr).unwrap();
        let objects = self.save_objects();

        snapshot_data.append(&mut  serde_cbor::to_vec(&objects).unwrap());
        Ok(snapshot_data)
    }

    pub fn save(&mut self, app_version: u16, description: String) -> std::io::Result<()> {
        let snapshot_data = self.save_to_mem(app_version, description).unwrap();

        let mut file = self.file.as_ref().unwrap();
        file.set_len(0)?;
        file.seek(SeekFrom::Start(0))?;

        file.write(&snapshot_data)?;
        file.sync_all()?;

        Ok(())
    }

    pub fn load_from_mem(mem: &[u8]) -> std::io::Result<Snapshot> {
        let mut snapshot_engine = Snapshot {
            objects: HashMap::new(),
            file: None,
        };

        let mut deserializer = Deserializer::from_slice(&mem);

        // Load the snapshot header.
        let _hdr: SnapshotHdr = serde::de::Deserialize::deserialize(&mut deserializer).unwrap();
        let objects: Vec<SnapshotObject> = serde::de::Deserialize::deserialize(&mut deserializer).unwrap();
        snapshot_engine.load_objects(objects);
        
        Ok(snapshot_engine)
    }

    pub fn load(path: &Path) -> std::io::Result<Snapshot> {
        let mut file = OpenOptions::new().read(true).write(true).open(path)?;
        let mut file_slice = Vec::new();
        file.read_to_end(&mut file_slice)?;

        Self::load_from_mem(file_slice.as_slice())
    }

    /// Restore an object with specified id and type.
    pub fn restore_object<D>(&mut self, id: String) -> D
    where
        D: Snapshotable + 'static,
    {
        D::restore(id, self)
    }

    /// Store an object with specified id and type.
    pub fn store_object<S>(&mut self, id: String, version: u16, object: &S)
    where
        S: Snapshotable + 'static,
    {
        object.snapshot(id, version, self);
    }

    /// Low level fn to set a snapshot property. 
    pub fn set_object<T: serde::ser::Serialize + 'static>(
        &mut self,
        kind: SnapshotObjectType,
        id: String,
        version: u16,
        data: &T,
    ) {
        self.set_raw_object(kind, id, version, serde_cbor::to_vec(data).unwrap())
    }

    /// Low level fn to get a snapshot property. 
    pub fn get_object<T: serde::de::DeserializeOwned + 'static>(
        &mut self,
        id: String,
    ) -> Option<T> {
        self.get_raw_object(id).map(|snapshot_object| {
            let mut deserializer = Deserializer::from_slice(snapshot_object);
            let object: T = serde::de::Deserialize::deserialize(&mut deserializer).unwrap();
            object
        })
    }

    //// Internal APIs
    pub(crate) fn get_raw_object(&self, id: String) -> Option<&Vec<u8>> {
        if let Some(object) = self.objects.get(&id) {
            return Some(&object.data)
        }
        None
    }

    pub(crate) fn set_raw_object(
        &mut self,
        kind: SnapshotObjectType,
        id: String,
        version: u16,
        data: SnapshotBlob,
    ) {
        let object = SnapshotObject {
            kind,
            version,
            id: id.clone(),
            data
        };
        self.objects.insert(id, object);
    }

    // Returns the objects to be saved.
    fn save_objects(&mut self) -> Vec<&SnapshotObject> {
        let mut objects = Vec::new();

        for (_, snapshot_object) in &self.objects {
            objects.push(snapshot_object);
        }

        objects
    }

    fn load_objects(&mut self, objects: Vec<SnapshotObject>) {
        for object in objects {
            self.objects.insert(
                object.id.clone(),
                object
            );
        }
    }
}



mod tests {
    use super::*;
    include!("structs.rs");
    include!("/tmp/translator.rs");

    #[test]
    fn test_save() {
        let mut snapshot = Snapshot::new(Path::new("/tmp/snap.fcs")).unwrap();
        let p = Test_V1 {
            field1: 10,
            field2: "Andrei".to_owned(),
            field3: vec![1; 3],
        };

        println!("Saving struct as {:?}", &p);

        snapshot.store_object("test_object".to_owned(), 2, &p);
        snapshot.save(1, "Testing".to_owned()).unwrap();

        snapshot = Snapshot::load(Path::new("/tmp/snap.fcs")).unwrap();
        let x: Test_V3 = snapshot.restore_object("test_object".to_owned());
        let y: Test_V2 = snapshot.restore_object("test_object".to_owned());
        let z: Test_V1 = snapshot.restore_object("test_object".to_owned());

        println!("Restore as {:?}", x);
        println!("Restore as {:?}", y);
        println!("Restore as {:?}", z);

        assert!(false);
    }
}
