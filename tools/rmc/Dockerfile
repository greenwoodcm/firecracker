FROM ubuntu:20.04
ARG RMC_BRANCH=main-153-2021-07-02

# Install rmc dependencies
#
RUN apt-get --yes update \
  && DEBIAN_FRONTEND=noninteractive apt-get install --no-install-recommends --yes \
  bison \
  cmake \
  curl \
  flex \
  g++ \
  gcc \
  git \
  gpg-agent \
  libssl-dev \
  lsb-release \
  make \
  ninja-build \
  patch \
  pkg-config \
  python-is-python3 \
  python3-pip \
  software-properties-common \
  wget \
  zlib1g \
  zlib1g-dev \
  ctags

# Install CBMS
RUN wget https://github.com/diffblue/cbmc/releases/download/cbmc-5.30.1/ubuntu-20.04-cbmc-5.30.1-Linux.deb
RUN dpkg -i ubuntu-20.04-cbmc-5.30.1-Linux.deb

# Install rust
RUN curl https://sh.rustup.rs -sSf | sh -s -- -y

# Install CBMC viewer.
RUN git clone https://github.com/awslabs/aws-viewer-for-cbmc.git
RUN cd aws-viewer-for-cbmc && make develop

# Build RMC from Github source.
RUN git clone https://github.com/model-checking/rmc.git
# Branch is passed as a parameter.
RUN cd rmc && git checkout ${RMC_BRANCH}

# Build RMC.
RUN cd rmc && ./configure \
  --enable-debug \
  --set=llvm.download-ci-llvm=true \
  --set=rust.debug-assertions-std=false \
  --set=rust.deny-warnings=false && \
  ./x.py build -i --stage 1 library/std

# Add path to RMC tool and CBMC viewer.
ENV PATH "$PATH:/rmc/scripts:/aws-viewer-for-cbmc"

# Run RMC regression tests.
# RUN cd rmc && scripts/rmc-regression.sh

RUN rustup target add x86_64-unknown-linux-musl