use std::cmp::PartialEq;
use snapshot_derive::Snapshot;

#[derive(Snapshot, Debug, PartialEq)]
#[snapshot(version = 2)]
struct Test_V1 {
    #[snapshot(default = 100)]
    field1: u32,
    #[snapshot(default = "default")]
    field2: String,
    // Default value for this field is infered as an empty vec.
    field3: Vec<u8>
}

#[derive(Snapshot, Debug, PartialEq)]
#[snapshot(version = 3)]
struct Test_V2 {
    field1: u32,
    field2: String,
}

#[derive(Snapshot, Debug, PartialEq)]
#[snapshot(version = 4)]
struct Test_V3 {
    field1: u32,
    field2: String,
    #[snapshot(default = true)]
    is_cool: bool,
    nested: Test_inner
}

#[derive(Snapshot, Debug, PartialEq)]
#[snapshot(version = 1)]
struct Test_inner {
   inner: u64
}