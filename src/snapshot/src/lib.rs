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
///  |       SnapshotMetadata     |
///  |----------------------------|
///  |          DataBlob          |
///  |----------------------------|
///
///
/// The header contains snapshot format version, firecracker version
/// and a description string.
/// The metadata stores a vector of SnapshotObject entries which describe
/// the data contained in the datablob. Each property id is unique in its
/// SnapshotObjectType space. The version field indicates the property struct
/// version to be used when deserializing it. The offset and len fields refer
/// to the serialized struct location and size within the DataBlob.
///
/// The snapshot engine works as a data store, properties are created/read
/// using get/set_object.
///
/// Loading a snapshot does not trigger any version translation, it simply
/// loads all the metadata and uses it to create u8 slices for each property.
///

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
    // Object version
    version: u16,
    // Unique ID.
    id: String,
    kind: SnapshotObjectType,
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
    props: Vec<SnapshotObject>,
}

pub struct Snapshot {
    props: HashMap<(SnapshotObjectType, String), (u16, SnapshotBlob)>,
    file: File,
    data_blob: SnapshotBlob,
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
    pub fn new(path: &Path) -> std::io::Result<Snapshot> {
        let file = OpenOptions::new().create(true).write(true).open(path)?;

        Ok(Snapshot {
            props: HashMap::new(),
            file,
            data_blob: Vec::new(),
        })
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
        let _hdr: SnapshotHdr = serde::de::Deserialize::deserialize(&mut deserializer).unwrap();
        let metadata: SnapshotMetadata =
            serde::de::Deserialize::deserialize(&mut deserializer).unwrap();

        // Load the data blob.
        snapshot_engine.data_blob = file_slice[deserializer.byte_offset()..].to_vec();
        // We need the blob of data because next we will create blobs for each prop using
        // the data_blob slice.
        snapshot_engine.load_metadata(metadata);

        Ok(snapshot_engine)
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
        self.set_raw_property(kind, id, version, serde_cbor::to_vec(data).unwrap())
    }

    /// Low level fn to get a snapshot property. 
    pub fn get_object<T: serde::de::DeserializeOwned + 'static>(
        &mut self,
        kind: SnapshotObjectType,
        id: String,
    ) -> Option<T> {
        self.get_raw_property(kind, id).map(|blob| {
            let mut deserializer = Deserializer::from_slice(blob.as_slice());
            let prop: T = serde::de::Deserialize::deserialize(&mut deserializer).unwrap();
            prop
        })
    }

    //// Internal APIs
    pub(crate) fn get_raw_property(&self, kind: SnapshotObjectType, id: String) -> Option<&Vec<u8>> {
        self.props.get(&(kind, id)).map(|(_, blob)| blob)
    }

    pub(crate) fn set_raw_property(
        &mut self,
        kind: SnapshotObjectType,
        id: String,
        version: u16,
        blob: SnapshotBlob,
    ) {
        self.props.insert((kind, id), (version, blob));
    }

    // Returns data blob and metadata
    fn save_metadata(&mut self) -> (Vec<u8>, SnapshotMetadata) {
        let mut metadata = SnapshotMetadata { props: Vec::new() };

        let mut data_blob = Vec::new();

        for ((kind, id), prop_blob) in &self.props {
            let prop = SnapshotObject {
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
