// -------------------------------------------------------------------------------------------------------

use super::TransmaterializationError;

/// Safely convert a u64 to usize, returning an error if it overflows on the current platform.
pub fn u64_to_usize(value: u64) -> Result<usize, TransmaterializationError> {
    value
        .try_into()
        .map_err(|_| anyhow::anyhow!("value {} exceeds platform usize", value))
}

/// Safely convert an i64 to isize, returning an error if it overflows on the current platform.
pub fn i64_to_isize(value: i64) -> Result<isize, TransmaterializationError> {
    value
        .try_into()
        .map_err(|_| anyhow::anyhow!("value {} exceeds platform isize", value))
}

/// Read 8 bytes as little-endian u64 and convert to usize.
/// Returns an error if the slice is not exactly 8 bytes or the value overflows usize.
pub fn usize_from_u64_le_bytes(bytes: &[u8]) -> Result<usize, TransmaterializationError> {
    let arr: [u8; 8] = bytes
        .try_into()
        .map_err(|_| anyhow::anyhow!("expected 8 bytes, got {}", bytes.len()))?;
    u64_to_usize(u64::from_le_bytes(arr))
}

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
