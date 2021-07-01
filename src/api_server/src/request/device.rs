// Copyright 2020 Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::super::VmmAction;
use crate::parsed_request::{Error, ParsedRequest};
use crate::request::Body;
use vmm::vmm_config::device::VfioDeviceConfig;

pub(crate) fn parse_put_device(body: &Body) -> Result<ParsedRequest, Error> {
    Ok(ParsedRequest::new_sync(VmmAction::SetVfioDevice(
        serde_json::from_slice::<VfioDeviceConfig>(body.raw()).map_err(|e| Error::SerdeJson(e))?,
    )))
}
