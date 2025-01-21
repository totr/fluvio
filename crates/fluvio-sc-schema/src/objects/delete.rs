//!
//! # Delete object
//!
//!

use std::fmt::Debug;

use anyhow::Result;

use fluvio_protocol::{Encoder, Decoder, Version};
use fluvio_protocol::api::Request;

use crate::{DeletableAdminSpec, TryEncodableFrom};
use crate::Status;
use crate::AdminPublicApiKey;
use super::classic::{ClassicDeleteApiEnum, ClassicDecodingDelete};
use super::{COMMON_VERSION, TypeBuffer};

#[derive(Debug, Default, Encoder, Decoder)]
pub struct DeleteRequest<S: DeletableAdminSpec> {
    key: S::DeleteKey,
    #[fluvio(min_version = 13)]
    force: bool,
}

impl<S> DeleteRequest<S>
where
    S: DeletableAdminSpec,
{
    pub fn new(key: S::DeleteKey) -> Self {
        Self { key, force: false }
    }

    pub fn with(key: S::DeleteKey, force: bool) -> Self {
        Self { key, force }
    }

    pub fn key(self) -> S::DeleteKey {
        self.key
    }

    pub fn is_force(&self) -> bool {
        self.force
    }
}

// This can be auto generated by enum derive later
#[derive(Debug, Default, Encoder)]
pub struct ObjectApiDeleteRequest(TypeBuffer);

impl<S> TryEncodableFrom<DeleteRequest<S>> for ObjectApiDeleteRequest
where
    S: DeletableAdminSpec,
{
    fn try_encode_from(input: DeleteRequest<S>, version: Version) -> Result<Self> {
        Ok(Self(TypeBuffer::encode::<S, _>(input, version)?))
    }

    fn downcast(&self) -> Result<Option<DeleteRequest<S>>> {
        self.0.downcast::<S, _>()
    }
}

impl Request for ObjectApiDeleteRequest {
    const API_KEY: u16 = AdminPublicApiKey::Delete as u16;
    const MIN_API_VERSION: i16 = 1; // previous version
    const DEFAULT_API_VERSION: i16 = COMMON_VERSION;
    type Response = Status;
}

// below is for classic support, this should go away after we remove classic
ClassicDeleteApiEnum!(DeleteRequest);
ClassicDecodingDelete!(DeleteRequest);
