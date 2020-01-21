use std::cmp::PartialEq;
use snapshot_derive::Snapshot;

// use kvm::{
//     CpuId, Kvm, KvmArray, KvmMsrs, MsrList, VcpuExit, VcpuFd, VmFd, KVM_IRQCHIP_IOAPIC,
//     KVM_IRQCHIP_PIC_MASTER, KVM_IRQCHIP_PIC_SLAVE, MAX_KVM_CPUID_ENTRIES,
// };
// use kvm_bindings::kvm_userspace_memory_region;
// use kvm_bindings::{
//     kvm_clock_data, kvm_debugregs, kvm_irqchip, kvm_lapic_state, kvm_mp_state, kvm_pit_config,
//     kvm_pit_state2, kvm_regs, kvm_sregs, kvm_vcpu_events, kvm_xcrs, kvm_xsave,
//     KVM_PIT_SPEAKER_DUMMY,
// };
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

// #[derive(Snapshot, Debug, PartialEq)]
// #[snapshot(version = 1)]
// pub struct MmioDeviceState {
//     pub addr: u64,
//     pub irq: u32,

//     pub device_activated: bool,
//     pub features_select: u32,
//     pub acked_features_select: u32,
//     pub queue_select: u32,
//     pub interrupt_status: usize,
//     pub driver_status: u32,
//     pub config_generation: u32,
//     pub queues: Vec<Queue>,

//     pub generic_virtio_device_state: GenericVirtioDeviceState,
//     pub specific_virtio_device_state: SpecificVirtioDeviceState,
// }

// #[derive(Snapshot, Clone, PartialEq)]
// #[snapshot(version = 1)]
// pub struct GenericVirtioDeviceState {
//     device_type: u32,
//     device_id: String,
//     avail_features: u64,
//     acked_features: u64,
//     config_space: Vec<u8>,
// }

// #[derive(Snapshot, Clone, PartialEq)]
// pub enum SpecificVirtioDeviceState {
//     Balloon(BalloonState),
//     Block(BlockState),
//     Net(NetState),
// }
