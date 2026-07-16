use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "../wasm_client/dist/"]
pub struct Asset;
