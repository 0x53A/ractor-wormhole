use futures::{Sink, Stream};
use std::pin::Pin;

// -------------------------------------------------------------------------------------------------------

pub enum ConduitMessage {
    Text(String),
    Binary(Vec<u8>),
    Close(Option<String>),
}

pub type ConduitError = anyhow::Error;

pub type ConduitSink = Pin<Box<dyn Sink<ConduitMessage, Error = ConduitError> + Send>>;
pub type ConduitSource = Pin<Box<dyn Stream<Item = Result<ConduitMessage, ConduitError>> + Send>>;
