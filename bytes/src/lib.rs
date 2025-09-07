pub const fn find_index_of_element_const(src: &[u8], byte: u8) -> Option<usize> {
    let (mut iter_index, mut found_index) = (0, None);

    while iter_index < src.len() {
        if src[iter_index] == byte {
            found_index = Some(iter_index);
            break;
        }

        iter_index += 1;
    }

    found_index
}

pub const fn splitln_const(src: &[u8]) -> Option<(&[u8], &[u8])> {
    let Some(next_newline_index) = find_index_of_element_const(src, b'\n') else {
        return None;
    };
    // We add one here, so that the newline we find is in
    // the current slice, and not the remainder
    let split_index = next_newline_index + 1;
    src.split_at_checked(split_index)
}

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

pub struct NewlineIter<'a>(Option<&'a [u8]>);

impl<'a> NewlineIter<'a> {
    #[inline]
    pub const fn new(src: &'a [u8]) -> NewlineIter<'a> {
        NewlineIter(Some(src))
    }
    /// Const compatible string-conversion constructor
    #[inline]
    #[allow(dead_code)]
    pub const fn from_str(src: &'a str) -> NewlineIter<'a> {
        let bytes = src.as_bytes();
        NewlineIter::new(bytes)
    }
}

impl<'a, T: AsRef<[u8]> + ?Sized> From<&'a T> for NewlineIter<'a> {
    fn from(src: &'a T) -> Self {
        let src: &'a [u8] = src.as_ref();
        NewlineIter::new(src)
    }
}

impl<'a> NewlineIter<'a> {
    pub const fn next_const(&mut self) -> Option<&'a [u8]> {
        // We use the internal option to track state,
        // if it is none, then we know we are `Fused`
        let Some(src) = self.0 else {
            return None;
        };

        let Some((next_line, remainder)) = splitln_const(src) else {
            // If we cannot find a new-line, the input may not be properly terminated.
            let last = src;
            // Set the fuse
            self.0 = None;
            // in this case, check if the last value is empty,
            // and return it only if it is not
            return if last.is_empty() { None } else { Some(last) };
        };

        self.0 = Some(remainder);

        Some(next_line)
    }
}

impl<'a> Iterator for NewlineIter<'a> {
    type Item = &'a [u8];
    fn next(&mut self) -> Option<Self::Item> {
        self.next_const()
    }
}

pub struct TabIter<'a>(Option<&'a [u8]>);

impl<'a> TabIter<'a> {
    pub const fn new(src: &'a [u8]) -> TabIter<'a> {
        TabIter(Some(src))
    }
}

impl<'a, T: AsRef<[u8]> + ?Sized> From<&'a T> for TabIter<'a> {
    fn from(src: &'a T) -> Self {
        let src: &'a [u8] = src.as_ref();
        TabIter::new(src)
    }
}

impl<'a> Iterator for TabIter<'a> {
    type Item = &'a [u8];
    fn next(&mut self) -> Option<Self::Item> {
        let src = self.0?;

        const TAB_SEQUENCE: &[u8] = b"-->8";
        let Some(index_of_tab_sequence) = src
            .windows(TAB_SEQUENCE.len())
            .enumerate()
            .find_map(|(idx, window)| window.eq(TAB_SEQUENCE).then_some(idx))
        else {
            self.0 = None;
            return Some(src);
        };

        let (tab_data, remainder) = src.split_at(index_of_tab_sequence);

        let (_tab_sequence, remainder) = remainder.split_at(TAB_SEQUENCE.len() + 1);
        self.0 = Some(remainder);

        Some(tab_data)
    }
}

impl core::iter::FusedIterator for NewlineIter<'_> {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newline_iter() {
        const TEST_DATA: &str = r"line 0
line 1
line 2
line 3
line 4
line 5
line 6
line 7
line 8
line 9
line a
line b
line c
line d
line e
line f";
        #[allow(dead_code)] // just cool
        const LINE_ONE_BYTES: Option<&'static [u8]> = NewlineIter::from_str(TEST_DATA).next_const();
        #[allow(dead_code)] // just cool
        const LINE_ONE_STR: Option<Result<&'static str, core::str::Utf8Error>> =
            match LINE_ONE_BYTES {
                Some(v) => Some(core::str::from_utf8(v)),
                None => None,
            };
        let nl_iter = NewlineIter::from(TEST_DATA);
        for (iter_index, line) in nl_iter.enumerate() {
            let line = core::str::from_utf8(line).unwrap();
            println!("parsing line {line:?}");
            // Assert the format of each line
            let ("line ", digit) = line.split_at("line ".len()) else {
                panic!()
            };
            let parsed_line_index = usize::from_str_radix(digit.trim_end(), 16).unwrap();
            assert_eq!(iter_index, parsed_line_index)
        }
    }
}
