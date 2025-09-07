/// Returns the index of the sequence if it can be found
#[tracing::instrument(level = "debug", skip(bytes, seq), ret)]
pub fn find_sequence(bytes: &[u8], seq: &[u8]) -> Option<usize> {
    tracing::debug!(
        "Searching {} bytes for sequence {:?}",
        bytes.len(),
        core::str::from_utf8(seq)
    );
    bytes
        .windows(seq.len())
        .enumerate()
        .find_map(|(seq_idx, window)| window.eq(seq).then_some(seq_idx))
}

/// Makes sure the sequence is removed from the bytes
pub fn split_at_sequence_exclusive<'a>(
    bytes: &'a [u8],
    seq: &[u8],
) -> Option<(&'a [u8], &'a [u8])> {
    find_sequence(bytes, seq).and_then(|seq_idx| bytes.split_at_checked(seq_idx + seq.len()))
}
