use super::SnapshotPropKind;
use serde::de::DeserializeOwned;
use serde::ser::Serialize;
use serde_cbor::{from_slice, to_vec, Deserializer};
use serde_derive::{Deserialize, Serialize};

pub struct State<S> {
    pub(crate) id: String,
    pub(crate) kind: SnapshotPropKind,
    pub(crate) version: u16,
    pub(crate) data: S,
}

/// Provides facilities for integrating with the snapshot engine.
pub trait SnapshotAdapter<S, D>
where
    S: Serialize + 'static,
    D: DeserializeOwned + 'static,
{
    fn load_state(&mut self, state: D);
    fn save_state(&self) -> State<S>;
}
