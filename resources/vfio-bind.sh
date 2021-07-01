#!/bin/bash

## Load vfio
modprobe vfio
modprobe vfio_iommu_type1
modprobe vfio_pci

die () {
    echo >&2 "$@"
    exit 1
}

[ "$#" -eq 1 ] || die "Please specify device BDF"

BDF=$1
VENDOR=`cat /sys/bus/pci/devices/$BDF/vendor`
DEVICE=`cat /sys/bus/pci/devices/$BDF/device`
DRIVER="/sys/bus/pci/devices/$BDF/driver"

echo Unbinding $BDF ...
echo $BDF > $DRIVER/unbind
echo Binding $BDF to vfio-pci driver
echo "vfio-pci" > /sys/bus/pci/devices/$BDF/driver_override
echo $BDF > /sys/bus/pci/drivers_probe