#![feature(ptr_metadata)]
use derive_dst::DST;

#[repr(C)]
#[expect(nonstandard_style, reason = "mirroring C API")]
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Hash, DST)]
pub struct CStruct {
    headSize: u32,
    tailElements: u32,
    Utf16String: [u16],
}

impl CStruct {
    pub fn new(size: usize) -> Box<Self> {
        let (mut uninit, (layout, tail_offset, tail_count)) =
            Self::new_uninit_for_size(size).unwrap();

        assert_eq!(layout.size(), size);

        uninit.headSize.write(tail_offset as u32);
        uninit.tailElements.write(tail_count as u32);
        
        unsafe { uninit.assume_init() }
    }

    pub fn to_utf8(&self) -> String {
        String::from_utf16(&self.Utf16String).unwrap()
    }

    pub fn set_string(&mut self, s: &str) {
        let utf16: Vec<u16> = s.encode_utf16().collect();
        self.Utf16String.copy_from_slice(utf16.as_slice());
    }
}

fn main() {}
