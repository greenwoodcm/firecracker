#!/bin/bash

FC_DIR=$(cd "$(dirname "$0")" && pwd)/../..

# Spawn shell
docker run \
    --volume "$FC_DIR:/firecracker" \
    --workdir "/firecracker" \
    --rm --env PS1="$(whoami)@\h:\w\$ " -it rmc /bin/bash