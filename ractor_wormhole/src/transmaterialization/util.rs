// -------------------------------------------------------------------------------------------------------

use super::TransmaterializationError;

pub fn require_buffer_size(buffer: &[u8], size: usize) -> Result<(), TransmaterializationError> {
    if buffer.len() != size {
        return Err(anyhow::anyhow!(
            "Buffer size mismatch: expected {}, got {}",
            size,
            buffer.len()
        ));
    }
    Ok(())
}

pub fn require_min_buffer_size(
    buffer: &[u8],
    size: usize,
) -> Result<(), TransmaterializationError> {
    if buffer.len() < size {
        return Err(anyhow::anyhow!(
            "Buffer size mismatch: expected {}, got {}",
            size,
            buffer.len()
        ));
    }
    Ok(())
}
