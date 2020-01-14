// Copyright 2019 Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0
extern crate serde;
extern crate serde_cbor;
extern crate serde_derive;

use serde_cbor::{from_slice, to_vec, Deserializer};
use serde_derive::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

const SNAPSHOT_FORMAT_VERSION: u16 = 1;

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

pub struct SnapshotEngine {
    props: HashMap<(SnapshotPropKind, String), (u16, SnapshotBlob)>,
    file: File,
    data_blob: SnapshotBlob,
}

pub fn get_snapshot_property<T: serde::de::DeserializeOwned + 'static>(
    engine: &mut SnapshotEngine,
    kind: SnapshotPropKind,
    id: String,
) -> Option<T> {
    engine.get_raw_property(kind, id).map(|blob| {
        let mut deserializer = Deserializer::from_slice(blob.as_slice());
        let prop: T = serde::de::Deserialize::deserialize(&mut deserializer).unwrap();
        prop
    })
}

pub fn set_snapshot_property<T: serde::ser::Serialize + 'static> (
    engine: &mut SnapshotEngine,
    kind: SnapshotPropKind,
    id: String,
    version: u16,
    data: T
) {
    engine.set_raw_property(kind, id, version, serde_cbor::to_vec(&data).unwrap())
}

impl SnapshotEngine { 
    pub fn new(path: &Path) -> std::io::Result<SnapshotEngine> {
        let file = OpenOptions::new().create(true).write(true).open(path)?;

        Ok(SnapshotEngine {
            props: HashMap::new(),
            file,
            data_blob: Vec::new(),
        })
    }

    pub(crate) fn get_raw_property(&self, kind: SnapshotPropKind, id: String) -> Option<&Vec<u8>> {
        self.props.get(&(kind, id)).map(|(_, blob)| blob)
    }

    pub(crate) fn set_raw_property(&mut self, kind: SnapshotPropKind, id: String, version: u16, blob: SnapshotBlob) {
        self.props.insert((kind, id), (version, blob));
    }

    pub fn save(
        &mut self,
        app_version: u16,
        description: String,
    ) -> std::io::Result<()> {
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

    pub fn load(path: &Path) -> std::io::Result<SnapshotEngine> {
        let mut file = OpenOptions::new().read(true).write(true).open(path)?;
        let mut file_slice = Vec::new();
        file.read_to_end(&mut file_slice)?;


        let mut snapshot_engine = SnapshotEngine {
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
        // We need the blobl of data as this fn call will create blobs for each prop using
        // data_blob slice.
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
            self.props
                .insert((prop.kind, prop.id), (prop.version, self.data_blob[prop.offset .. prop.offset + prop.len].to_vec()));
        }
    }
}

mod tests {
    use super::*;

    #[derive(Debug, Serialize, Deserialize, Clone)]
    struct Person {
        age: u8,
        name: String
    }

    #[test]
    fn test_save() {
        let mut engine = SnapshotEngine::new(Path::new("/tmp/snap.fcs")).unwrap();

        let person = Person {
            age: 35,
            name: "Andrei".to_owned()
        };
        let person2 = Person {
            age: 33,
            name: "Georgiana".to_owned()
        };
        set_snapshot_property(&mut engine, SnapshotPropKind::CONFIG, "author".to_owned(), 1, person);
        set_snapshot_property(&mut engine, SnapshotPropKind::CONFIG, "wife".to_owned(), 1, person2);

        engine.save(1, "Testing".to_owned()).unwrap();

        engine = SnapshotEngine::load(Path::new("/tmp/snap.fcs")).unwrap();

        let p1: Person = get_snapshot_property(&mut engine, SnapshotPropKind::CONFIG, "author".to_owned()).unwrap();
        let p2: Person = get_snapshot_property(&mut engine, SnapshotPropKind::CONFIG, "wife".to_owned()).unwrap();

        println!("{:?}", p1);
        println!("{:?}", p2);

        assert!(false);
    }
}