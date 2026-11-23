#![feature(ptr_metadata)]
use derive_dst::DST;

#[repr(C)]
#[expect(nonstandard_style, reason = "mirroring C API")]
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Hash, DST)]
pub struct CStruct {
    headSize: u32,
    tailElements: u32,
    tailSize: u32,
    Utf16String: [u16],
}

impl CStruct {
    pub fn new(count: usize) -> Box<Self> {
        let (mut uninit, (layout, tail_offset, tail_count)) =
            Self::new_uninit_for_count(count).unwrap();

        assert_eq!(tail_count, count);

        uninit.headSize.write(tail_offset as u32);
        uninit
            .tailSize
            .write(layout.size() as u32 - tail_offset as u32);
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

#[test]
fn usage() {
    let text = "Hello dynamic world";
    let tail_elements = text.encode_utf16().count();
    assert_eq!(tail_elements, 19);
    let mut cstruct = CStruct::new(tail_elements);
    assert_eq!(cstruct.headSize, 12);
    assert_eq!(cstruct.tailElements as usize, 19);
    assert_eq!(cstruct.tailSize as usize, 40);
    assert_eq!(cstruct.Utf16String, [0_u16; 19]);
    cstruct.set_string(text);
    assert_eq!(cstruct.to_utf8(), text);
}
