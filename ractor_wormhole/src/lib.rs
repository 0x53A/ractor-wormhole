#![feature(fn_traits)]
#![feature(min_specialization)]

pub mod conduit;
pub mod nexus;
pub mod portal;
pub mod transmaterialization;
pub mod util;

extern crate self as ractor_wormhole;

// re-export the derive macro
#[cfg(feature = "derive")]
pub use ractor_wormhole_derive::WormholeTransmaterializable;

// re-export ractor itself
pub use ractor;

// re-export all direct dependencies
ractor_wormhole_derive::generate_deps!();
