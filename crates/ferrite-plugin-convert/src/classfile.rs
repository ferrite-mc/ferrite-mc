//! Minimal JVM `.class` constant-pool parser for static Bukkit/Paper/Folia/NMS API scanning.

use std::collections::BTreeSet;

/// A method invocation reference extracted from a JVM `.class` constant pool.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, serde::Serialize)]
pub struct MethodRef {
    pub owner: String,
    pub name: String,
    pub descriptor: String,
}

/// Extracted API references from one `.class` file.
#[derive(Debug, Default, Clone)]
pub struct ClassScan {
    pub class_refs: BTreeSet<String>,
    pub method_refs: BTreeSet<MethodRef>,
}

enum CpEntry {
    Empty,
    Utf8(String),
    Class(u16),
    NameAndType(u16, u16),
    MethodRef(u16, u16),
    Other,
}

struct Cursor<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, pos: 0 }
    }

    fn read_u8(&mut self) -> Option<u8> {
        let b = *self.bytes.get(self.pos)?;
        self.pos += 1;
        Some(b)
    }

    fn read_u16(&mut self) -> Option<u16> {
        let slice = self.bytes.get(self.pos..self.pos + 2)?;
        self.pos += 2;
        Some(u16::from_be_bytes([slice[0], slice[1]]))
    }

    fn read_u32(&mut self) -> Option<u32> {
        let slice = self.bytes.get(self.pos..self.pos + 4)?;
        self.pos += 4;
        Some(u32::from_be_bytes([slice[0], slice[1], slice[2], slice[3]]))
    }

    fn skip(&mut self, n: usize) -> Option<()> {
        if self.pos.checked_add(n)? > self.bytes.len() {
            return None;
        }
        self.pos += n;
        Some(())
    }

    fn read_bytes(&mut self, n: usize) -> Option<&'a [u8]> {
        let end = self.pos.checked_add(n)?;
        let slice = self.bytes.get(self.pos..end)?;
        self.pos = end;
        Some(slice)
    }
}

/// Scans a JVM `.class` byte buffer and extracts all class and method references from its
/// constant pool. Returns `None` if the buffer is not a valid class file.
pub fn scan_class_bytes(bytes: &[u8]) -> Option<ClassScan> {
    let mut cur = Cursor::new(bytes);
    let magic = cur.read_u32()?;
    if magic != 0xCAFE_BABE {
        return None;
    }
    let _minor = cur.read_u16()?;
    let _major = cur.read_u16()?;
    let cp_count = usize::from(cur.read_u16()?);
    if cp_count == 0 {
        return None;
    }

    let mut entries = Vec::with_capacity(cp_count);
    entries.push(CpEntry::Empty);

    let mut i = 1;
    while i < cp_count {
        let tag = cur.read_u8()?;
        match tag {
            1 => {
                // CONSTANT_Utf8
                let len = usize::from(cur.read_u16()?);
                let raw = cur.read_bytes(len)?;
                entries.push(CpEntry::Utf8(String::from_utf8_lossy(raw).into_owned()));
            }
            3 | 4 => {
                // Integer, Float
                cur.skip(4)?;
                entries.push(CpEntry::Other);
            }
            5 | 6 => {
                // Long, Double (occupies two constant-pool slots)
                cur.skip(8)?;
                entries.push(CpEntry::Other);
                entries.push(CpEntry::Empty);
                i += 1;
            }
            7 => {
                // Class
                let name_idx = cur.read_u16()?;
                entries.push(CpEntry::Class(name_idx));
            }
            8 | 16 | 19 | 20 => {
                // String, MethodType, Module, Package
                cur.skip(2)?;
                entries.push(CpEntry::Other);
            }
            9 => {
                // Fieldref
                cur.skip(4)?;
                entries.push(CpEntry::Other);
            }
            10 | 11 => {
                // Methodref, InterfaceMethodref
                let class_idx = cur.read_u16()?;
                let nat_idx = cur.read_u16()?;
                entries.push(CpEntry::MethodRef(class_idx, nat_idx));
            }
            12 => {
                // NameAndType
                let name_idx = cur.read_u16()?;
                let desc_idx = cur.read_u16()?;
                entries.push(CpEntry::NameAndType(name_idx, desc_idx));
            }
            15 => {
                // MethodHandle
                cur.skip(3)?;
                entries.push(CpEntry::Other);
            }
            17 | 18 => {
                // Dynamic, InvokeDynamic
                cur.skip(4)?;
                entries.push(CpEntry::Other);
            }
            _ => return None,
        }
        i += 1;
    }

    let utf8 = |idx: u16| -> Option<&str> {
        match entries.get(usize::from(idx)) {
            Some(CpEntry::Utf8(s)) => Some(s.as_str()),
            _ => None,
        }
    };

    let class_name = |idx: u16| -> Option<String> {
        match entries.get(usize::from(idx)) {
            Some(CpEntry::Class(name_idx)) => utf8(*name_idx).map(|s| s.replace('/', ".")),
            _ => None,
        }
    };

    let mut scan = ClassScan::default();
    for entry in &entries {
        match entry {
            CpEntry::Class(name_idx) => {
                if let Some(raw) = utf8(*name_idx)
                    && !raw.starts_with('[')
                {
                    scan.class_refs.insert(raw.replace('/', "."));
                }
            }
            CpEntry::MethodRef(class_idx, nat_idx) => {
                if let (Some(owner), Some(CpEntry::NameAndType(n_idx, d_idx))) =
                    (class_name(*class_idx), entries.get(usize::from(*nat_idx)))
                    && let (Some(name), Some(descriptor)) = (utf8(*n_idx), utf8(*d_idx))
                {
                    scan.method_refs.insert(MethodRef {
                        owner,
                        name: name.to_owned(),
                        descriptor: descriptor.to_owned(),
                    });
                }
            }
            _ => {}
        }
    }

    Some(scan)
}
