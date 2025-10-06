#![feature(fn_traits)]
#![feature(try_find)]
#![feature(negative_impls)]
#![feature(min_specialization)]
#![feature(macro_metavar_expr_concat)]

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