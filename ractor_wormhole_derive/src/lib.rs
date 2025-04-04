#![feature(cfg_boolean_literals)]

mod derive_wormhole_serializable;
mod util;

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;

use derive_wormhole_serializable::derive_wormhole_serializable_impl;

#[proc_macro_derive(WormholeSerializable, attributes(serde, bincode))]
pub fn derive_wormhole_serializable(input: TokenStream) -> TokenStream {
    let result = derive_wormhole_serializable_impl(TokenStream2::from(input));
    match result {
        Ok(ts) => TokenStream::from(ts),
        Err(e) => TokenStream::from(e.to_compile_error()),
    }
}
