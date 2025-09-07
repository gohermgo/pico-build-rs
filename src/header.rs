use core::borrow::Borrow;
use core::fmt;
use core::mem::transmute;
use core::ops::Deref;

use crate::bytes;

// Type definitions,
// conversion constructors,
// and boilerplate for Owned/Borrow

#[repr(transparent)]
pub struct Header([u8]);

impl AsRef<[u8]> for Header {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

impl Header {
    pub fn copy_to_boxed_slice(&self) -> Box<[u8]> {
        let Header(bytes) = self;
        Box::from(bytes)
    }
    /// # Safety
    /// This function is primarly inteded for slice-conversion
    /// once the data has already been correctly accessed and
    /// parsed from a .p8 file
    ///
    /// Using this function incorrectly leads to other invariants
    /// about this type no longer being guarantees.
    #[inline]
    pub const unsafe fn from_slice(src: &[u8]) -> &Header {
        unsafe { transmute(src) }
    }
}

impl ToOwned for Header {
    type Owned = HeaderBuf;
    fn to_owned(&self) -> Self::Owned {
        HeaderBuf::from(self.copy_to_boxed_slice())
    }
}

#[repr(transparent)]
pub struct HeaderBuf(Box<[u8]>);

impl From<Box<[u8]>> for HeaderBuf {
    fn from(value: Box<[u8]>) -> Self {
        HeaderBuf(value)
    }
}

impl Deref for HeaderBuf {
    type Target = Header;
    fn deref(&self) -> &Self::Target {
        // SAFETY: Provided this buffered variant of the
        // header was correctly parsed and acquired,
        // this operation remains safe according to
        // type-invariant guarantees
        unsafe { Header::from_slice(self.0.as_ref()) }
    }
}

impl Borrow<Header> for HeaderBuf {
    fn borrow(&self) -> &Header {
        self
    }
}

#[repr(transparent)]
pub struct Version([u8]);
impl Version {
    // #[tracing::instrument(level = "debug", ret)]
    pub fn parse(&self) -> Result<usize, <usize as core::str::FromStr>::Err> {
        // SAFETY: This version-line will always be utf-8 according to pico-8 spec
        let src = unsafe { core::str::from_utf8_unchecked(&self.0) };
        let ("version ", version_number_string) = src.split_at("version ".len()) else {
            panic!("malformed version encountered");
        };
        version_number_string
            // There will be trailing newline
            .trim_end()
            .parse()
            .inspect_err(|e| tracing::error!("failed to parse version-number {e}"))
    }
    /// # Safety
    /// This function is primarly inteded for slice-conversion
    /// once the data has already been correctly accessed and
    /// parsed from a .p8 file
    ///
    /// Using this function incorrectly leads to other invariants
    /// about this type no longer being guarantees.
    #[inline]
    pub const unsafe fn from_slice(src: &[u8]) -> &Version {
        unsafe { transmute(src) }
    }
}

impl fmt::Debug for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Ok(num) = self.parse() {
            f.debug_tuple("Version").field(&num).finish()
        } else {
            f.debug_tuple("[unparsed]Version").field(&&self.0).finish()
        }
    }
}

const CARTRIDGE_MARKER: &[u8] = b"pico-8 cartridge // http://www.pico-8.com\n";

// Main header implementation
//

impl Header {
    fn get_as_tuple(&self) -> Option<(&[u8], &Version)> {
        let (CARTRIDGE_MARKER, version) = self
            .0
            .split_at_checked(CARTRIDGE_MARKER.len())
            .map(|(marker, version)| (marker, unsafe { Version::from_slice(version) }))?
        else {
            panic!("encountered malformed cartridge marker in header");
        };
        Some((CARTRIDGE_MARKER, version))
    }
    pub fn get_version(&self) -> Option<&Version> {
        self.get_as_tuple().map(|(_, v)| v)
    }
}

impl fmt::Debug for Header {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(version) = self.get_version() {
            f.debug_struct("Header")
                .field("version", &version)
                // Cartridge-marker will always be identical
                .finish_non_exhaustive()
        } else {
            f.debug_tuple("Header").field(&&self.0).finish()
        }
    }
}
impl fmt::Debug for HeaderBuf {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let h: &Header = self;
        h.fmt(f)
    }
}

#[tracing::instrument(level = "debug", skip(src))]
pub fn split_from<T: AsRef<[u8]> + ?Sized>(src: &T) -> Option<(&Header, &[u8])> {
    // Stash slice for later use
    let slice = src.as_ref();

    // Make iterator for taking some lines off
    let mut nl_iter = bytes::NewlineIter::new(slice);

    // Assert marker
    let CARTRIDGE_MARKER = nl_iter.next_const()? else {
        panic!("encountered malformed cartridge marker in header");
    };

    // Check if version is valid utf-8 (minimal correctness check)
    let version = nl_iter.next_const()?;
    if core::str::from_utf8(version).is_err() {
        tracing::warn!("Invalid utf-8 in version-bytes: {version:?}");
        return None;
    }

    let header_len = CARTRIDGE_MARKER.len() + version.len();
    let (slice, remainder) = slice.split_at(header_len);

    let header = unsafe { Header::from_slice(slice) };
    tracing::debug!("{header:?}")
    Some((header, remainder))
}
