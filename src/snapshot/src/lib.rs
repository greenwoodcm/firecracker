// Copyright 2020 Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0
extern crate serde;
extern crate serde_cbor;
extern crate serde_derive;

pub mod adapter;

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
///  |       SnapshotMetadata     |
///  |----------------------------|
///  |          DataBlob          |
///  |----------------------------|
/// 
/// 
/// The header contains snapshot format version, firecracker version
/// and a description string.
/// The metadata stores a vector of SnapshotProp entries which describe
/// the data contained in the datablob. Each property id is unique in its 
/// SnapshotPropKind space. The version field indicates the property struct 
/// version to be used when deserializing it. The offset and len fields refer
/// to the serialized struct location and size within the DataBlob.
/// 
/// The snapshot engine works as a data store, properties are created/read
/// using get/set_snapshot_property or using the SnapshotAdapter trait.
/// 
/// Loading a snapshot does not trigger any version translation, it simply
/// loads all the metadata and uses it to create u8 slices for each property.
/// 

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SnapshotPropKind {
    CONFIG,
    DEVICE,
}

type SnapshotBlob = Vec<u8>;

#[derive(Debug, Serialize, Deserialize, Clone)]
/// Describes a snapshot property.
pub struct SnapshotProp {
    // Struct version
    version: u16,
    // Unique ID.
    id: String,
    kind: SnapshotPropKind,
    // Offset inside the SnapshotData blob.
    offset: usize,
    // Length of the blob
    len: usize,
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

#[derive(Debug, Serialize, Deserialize)]
struct SnapshotMetadata {
    props: Vec<SnapshotProp>,
}

pub struct Snapshot {
    props: HashMap<(SnapshotPropKind, String), (u16, SnapshotBlob)>,
    file: File,
    data_blob: SnapshotBlob,
}

/// Trait that provides an implementation to deconstruct structs
/// into typed fields backed by the Snapshot storage.
/// This trait is automatically implemented on user specified structs
/// or can otherwise be implemented manually. 
pub trait Deconstruct {
    fn deconstruct(&self, id: String, engine: &mut Snapshot);
}

// Trait that provides an implementation to reconstruct structs
// from typed fields backed by the Snapshot storage.
// This trait is automatically implemented on user specified structs
// or can otherwise be implemented manually. 
pub trait Reconstruct {
    fn reconstruct(id: String, engine: &mut Snapshot) -> Self;
}

impl Snapshot {
    pub fn new(path: &Path) -> std::io::Result<Snapshot> {
        let file = OpenOptions::new().create(true).write(true).open(path)?;

        Ok(Snapshot {
            props: HashMap::new(),
            file,
            data_blob: Vec::new(),
        })
    }

    pub fn restore<D>(&mut self, id: String) -> D
    where
        D: Reconstruct + 'static 
    {
        D::reconstruct(id, self)
    }

    pub fn store<S>(&mut self, id: String, object: S)
    where
        S: Deconstruct + 'static 
    {
        object.deconstruct(id, self);
    }

    // Save the state of an object using a SnapshotAdapter interface.
    pub fn save_state<S, D>(&mut self, object: &adapter::SnapshotAdapter<S, D>)
    where
        S: serde::ser::Serialize + 'static,
        D: serde::de::DeserializeOwned + 'static,
    {
        let state = object.save_state();
        self.set_snapshot_property(state.kind, state.id, state.version, state.data);
    }

    pub fn set_snapshot_property<T: serde::ser::Serialize + 'static>(
        &mut self,
        kind: SnapshotPropKind,
        id: String,
        version: u16,
        data: T,
    ) {
        self.set_raw_property(kind, id, version, serde_cbor::to_vec(&data).unwrap())
    }

    pub fn get_snapshot_property<T: serde::de::DeserializeOwned + 'static>(
        &mut self,
        kind: SnapshotPropKind,
        id: String,
    ) -> Option<T> {
        self.get_raw_property(kind, id).map(|blob| {
            let mut deserializer = Deserializer::from_slice(blob.as_slice());
            let prop: T = serde::de::Deserialize::deserialize(&mut deserializer).unwrap();
            prop
        })
    }

    pub(crate) fn get_raw_property(&self, kind: SnapshotPropKind, id: String) -> Option<&Vec<u8>> {
        self.props.get(&(kind, id)).map(|(_, blob)| blob)
    }

    pub(crate) fn set_raw_property(
        &mut self,
        kind: SnapshotPropKind,
        id: String,
        version: u16,
        blob: SnapshotBlob,
    ) {
        self.props.insert((kind, id), (version, blob));
    }

    pub fn save(&mut self, app_version: u16, description: String) -> std::io::Result<()> {
        self.file.set_len(0)?;
        self.file.seek(SeekFrom::Start(0))?;

        // Serialize the header.
        let hdr = SnapshotHdr {
            version: SNAPSHOT_FORMAT_VERSION,
            data_version: app_version,
            description,
        };

        let mut snapshot_data = serde_cbor::to_vec(&hdr).unwrap();
        let (mut blob, metadata) = self.save_metadata();

        snapshot_data.append(&mut serde_cbor::to_vec(&metadata).unwrap());
        snapshot_data.append(&mut blob);
        self.file.write(&snapshot_data)?;
        self.file.sync_all()?;

        Ok(())
    }

    pub fn load(path: &Path) -> std::io::Result<Snapshot> {
        let mut file = OpenOptions::new().read(true).write(true).open(path)?;
        let mut file_slice = Vec::new();
        file.read_to_end(&mut file_slice)?;

        let mut snapshot_engine = Snapshot {
            props: HashMap::new(),
            file,
            data_blob: Vec::new(),
        };

        let mut deserializer = Deserializer::from_slice(&file_slice);

        // Load the snapshot header.
        let hdr: SnapshotHdr = serde::de::Deserialize::deserialize(&mut deserializer).unwrap();
        let metadata: SnapshotMetadata =
            serde::de::Deserialize::deserialize(&mut deserializer).unwrap();

        // Load the data blob.
        snapshot_engine.data_blob = file_slice[deserializer.byte_offset()..].to_vec();
        // We need the blob of data because next we will create blobs for each prop using
        // the data_blob slice.
        snapshot_engine.load_metadata(metadata);

        Ok(snapshot_engine)
    }

    // Returns data blob and metadata
    fn save_metadata(&mut self) -> (Vec<u8>, SnapshotMetadata) {
        let mut metadata = SnapshotMetadata { props: Vec::new() };

        let mut data_blob = Vec::new();

        for ((kind, id), prop_blob) in &self.props {
            let prop = SnapshotProp {
                version: prop_blob.0,
                id: id.to_string(),
                kind: *kind,
                offset: data_blob.len(),
                len: prop_blob.1.len(),
            };

            data_blob.append(&mut prop_blob.1.to_vec());
            metadata.props.push(prop);
        }

        (data_blob, metadata)
    }

    fn load_metadata(&mut self, metadata: SnapshotMetadata) {
        for prop in metadata.props {
            self.props.insert(
                (prop.kind, prop.id),
                (
                    prop.version,
                    self.data_blob[prop.offset..prop.offset + prop.len].to_vec(),
                ),
            );
        }
    }
}

#[derive(Serialize, Debug, Deserialize, Clone)]
pub struct Person {
    age: u8,
    name: String,
    // Field added in latest version(2)
    rank: u8
}

mod tests {
    use super::*;

    struct Dummy {}

    include!("/tmp/translator.rs");

    #[derive(Serialize, Debug, Deserialize, Clone)]
    struct Person_v1 {
        age: u8,
        name: String,
    }
    
    #[derive(Serialize, Debug, Deserialize, Clone)]
    struct Person_v3 {
        age: u8,
        name: String,
        rank: u8,
        child: Person,
    }

    impl adapter::SnapshotAdapter<Person, Person> for Dummy {
        fn load_state(&mut self, state: Person) {
            
        }

        fn save_state(&self) -> adapter::State<Person> {
            let person = Person {
                age: 133,
                name: "Santa".to_owned(),
                rank: 1
            };

            adapter::State {
                id: "DummyPerson".to_owned(),
                kind: SnapshotPropKind::DEVICE,
                version: 1,
                data: person
            }
        }
    }

    impl Deconstruct for Person {
        fn deconstruct(&self, id: String, engine: &mut Snapshot) {
            engine.set_snapshot_property(SnapshotPropKind::CONFIG, id.clone() + ".age", 0, self.age);
            engine.set_snapshot_property(SnapshotPropKind::CONFIG, id.clone() + ".name", 0, self.name.clone());
            engine.set_snapshot_property(SnapshotPropKind::CONFIG, id + ".rank", 0, self.rank);
        }
    }

    impl Reconstruct for Person {
        fn reconstruct(id: String, engine: &mut Snapshot) -> Self {
            Person {
                age: engine.get_snapshot_property(SnapshotPropKind::CONFIG, id.clone() + ".age").unwrap_or_default(),
                name: engine.get_snapshot_property(SnapshotPropKind::CONFIG, id.clone() + ".name").unwrap_or_default(),
                rank: engine.get_snapshot_property(SnapshotPropKind::CONFIG, id + ".rank").unwrap_or_default(),
            }
        }
    }

    #[test]
    fn test_save() {
        let mut engine = Snapshot::new(Path::new("/tmp/snap.fcs")).unwrap();
        let p = Person {
            age: 10,
            name: "Andrei".to_owned(),
            rank: 13,
        };


        // let person = Person_v1 {
        //     age: 35,
        //     name: "Andrei".to_owned(),
        // };

        // let person2 = Person_v1 {
        //     age: 33,
        //     name: "Georgiana".to_owned(),
        // };

        // engine.set_snapshot_property(
        //     SnapshotPropKind::CONFIG,
        //     "author".to_owned(),
        //     1,
        //     person,
        // );
        // engine.set_snapshot_property(
        //     SnapshotPropKind::CONFIG,
        //     "wife".to_owned(),
        //     1,
        //     person2,
        // );


        // engine.save_state(&Dummy {});
        
        
        engine.store("Person_1".to_owned(), p);
        engine.save(1,"Testing".to_owned()).unwrap();
        engine = Snapshot::load(Path::new("/tmp/snap.fcs")).unwrap();

        let x: Person = engine.restore("Person_1".to_owned());

        println!("{:?}", x);

        assert!(false);
    }
}
