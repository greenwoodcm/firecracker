#!/bin/bash

RMC_BRANCH=main-153-2021-07-02

if [ ! -z "$1" ]; then 
    RMC_BRANCH=$1 
fi

docker build -t rmc:latest . --build-arg RMC_BRANCH=$RMC_BRANCH