#[derive(rust_embed::Embed)]
#[folder = "../wasm_client/dist/"]
#[exclude(".stage/*")]
pub struct Asset;
