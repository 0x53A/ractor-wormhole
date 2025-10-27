mod derive_generate_deps;
mod derive_wormhole_serializable;
mod util;

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;

use derive_wormhole_serializable::derive_wormhole_serializable_impl;

#[proc_macro_derive(WormholeTransmaterializable, attributes(serde, bincode))]
pub fn derive_wormhole_transmaterializable(input: TokenStream) -> TokenStream {
    let result = derive_wormhole_serializable_impl(TokenStream2::from(input));
    match result {
        Ok(ts) => TokenStream::from(ts),
        Err(e) => TokenStream::from(e.to_compile_error()),
    }
}

#[proc_macro]
pub fn generate_deps(input: TokenStream) -> TokenStream {
    derive_generate_deps::generate_deps(input)
}
