use std::cmp::PartialEq;
use snapshot_derive::Snapshot;

#[derive(Snapshot, Debug, PartialEq)]
#[snapshot(version = 1)]
struct Test_v1 {
    #[snapshot(default = 100)]
    field1: u32,
    #[snapshot(default = "default")]
    field2: String,
    // Default value for this field is infered as an empty vec.
    field3: Vec<u8>
}

#[derive(Snapshot, Debug, PartialEq)]
struct Test_v2 {
    field1: u32,
    field2: String,
}

#[derive(Snapshot, Debug, PartialEq)]
struct Test_v3 {
    field1: u32,
    field2: String,
    field4: Vec<u8>
}
