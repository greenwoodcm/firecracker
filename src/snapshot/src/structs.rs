use std::cmp::PartialEq;
use snapshot_derive::Snapshot;

#[derive(Snapshot, Serialize, Debug, PartialEq)]
#[snapshot(version = 1)]
pub struct MmioDeviceState_v1 {
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
}

#[derive(Snapshot, Serialize, Debug, PartialEq)]
#[snapshot(version = 2)]
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
    #[snapshot(default = 128)]
    pub flag: u8
}

// #[derive(Snapshot, Serialize, Debug, PartialEq)]
// #[snapshot(version = 2)]
// struct Test_V1 {
//     #[snapshot(default = 100)]
//     field1: u32,
//     #[snapshot(default = "default")]
//     field2: String,
//     // Default value for this field is infered as an empty vec.
//     field3: Vec<u8>
// }

// #[derive(Snapshot, Serialize, Debug, PartialEq)]
// #[snapshot(version = 3)]
// struct Test_V2 {
//     field1: u32,
//     field2: String,
// }

// #[derive(Snapshot, Serialize, Debug, PartialEq)]
// #[snapshot(version = 4)]
// struct Test_V3 {
//     field1: u32,
//     field2: String,
//     #[snapshot(default = true)]
//     is_cool: bool,
// }

