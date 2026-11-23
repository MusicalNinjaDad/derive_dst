#![feature(ptr_metadata)]
use std::io;

use derive_dst::DST;

#[repr(C)]
#[derive(Debug, DST, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[allow(nonstandard_style, reason = "mirroring FFI struct")]
/// A dynamically sized ffi-compatible version of `SP_DEVICE_INTERFACE_DETAIL_DATA_W`
pub struct DeviceDetails {
    cbSize: u32,
    DevicePath: [u16],
}

impl DeviceDetails {
    /// Create a new DeviceDetails of the given size, as returned by an null-call to
    /// `SetupDiGetDeviceInterfaceDetailW`.
    ///
    /// Errors if the requested size does not produce a valid, well-aligned & padded struct
    pub fn new(size: usize) -> io::Result<Box<Self>> {
        // Create a new, boxed uninitialised DeviceDetails. This may return a LayoutError if the
        // overall struct is somehow invalid.
        let (mut raw, (layout, tail_offset, _tail_count)) =
            Self::new_uninit_for_size(size).map_err(io::Error::other)?;

        // Check that the returned DST is of the exact size requested. DST will always be
        // well-aligned and padded.
        if layout.size() != size {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid buffer size",
            ));
        }

        // API requires cbSize to be set to the size of the struct as if it were storing a single
        // `\0` as the DevicePath.
        let cbsize = tail_offset + size_of::<u16>();
        raw.cbSize.write(cbsize as u32);

        // SAFETY:
        // - cbSize is correctly set to size required for an empty path with a single null-terminator
        // - raw was initialized zeroed and `\0` is a valid value for u16
        let valid = unsafe { raw.assume_init() };
        Ok(valid)
    }
}

fn main() {}
