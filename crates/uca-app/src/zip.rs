// SPDX-FileCopyrightText: The UCA contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A store-only (uncompressed) zip writer for diagnostics bundles (11,
//! decided: no zip dependency). Entry names are UTF-8 with `/` separators.

use std::io::{self, Write};
use std::time::SystemTime;

const LOCAL: u32 = 0x0403_4b50;
const CENTRAL: u32 = 0x0201_4b50;
const END: u32 = 0x0605_4b50;
/// Version 2.0: plain stored entries.
const VERSION: u16 = 20;
/// Bit 11: names are UTF-8.
const UTF8: u16 = 0x0800;

struct Entry {
    name: String,
    crc: u32,
    size: u32,
    time: u16,
    date: u16,
    offset: u32,
}

pub struct ZipWriter<W: Write> {
    out: W,
    written: u64,
    entries: Vec<Entry>,
}

fn too_big(what: &str) -> io::Error {
    io::Error::other(format!("{what} is too big for a plain zip (4 GB)"))
}

impl<W: Write> ZipWriter<W> {
    pub fn new(out: W) -> Self {
        Self {
            out,
            written: 0,
            entries: Vec::new(),
        }
    }

    fn put(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.out.write_all(bytes)?;
        self.written += bytes.len() as u64;
        Ok(())
    }

    pub fn add(&mut self, name: &str, data: &[u8], modified: SystemTime) -> io::Result<()> {
        if self.entries.len() >= usize::from(u16::MAX) {
            return Err(io::Error::other("too many files for a plain zip"));
        }
        let size = u32::try_from(data.len()).map_err(|_| too_big(name))?;
        let offset = u32::try_from(self.written).map_err(|_| too_big("the bundle"))?;
        let name_len = u16::try_from(name.len()).map_err(|_| io::Error::other("name too long"))?;
        let (time, date) = dos_time(modified);
        let crc = crc32(data);
        let mut h = Vec::with_capacity(30 + name.len());
        h.extend(LOCAL.to_le_bytes());
        h.extend(VERSION.to_le_bytes());
        h.extend(UTF8.to_le_bytes());
        h.extend(0u16.to_le_bytes()); // stored
        h.extend(time.to_le_bytes());
        h.extend(date.to_le_bytes());
        h.extend(crc.to_le_bytes());
        h.extend(size.to_le_bytes());
        h.extend(size.to_le_bytes());
        h.extend(name_len.to_le_bytes());
        h.extend(0u16.to_le_bytes()); // no extra field
        h.extend(name.as_bytes());
        self.put(&h)?;
        self.put(data)?;
        self.entries.push(Entry {
            name: name.to_owned(),
            crc,
            size,
            time,
            date,
            offset,
        });
        Ok(())
    }

    /// Writes the central directory and returns the output.
    pub fn finish(mut self) -> io::Result<W> {
        let start = u32::try_from(self.written).map_err(|_| too_big("the bundle"))?;
        let entries = std::mem::take(&mut self.entries);
        for e in &entries {
            let mut c = Vec::with_capacity(46 + e.name.len());
            c.extend(CENTRAL.to_le_bytes());
            c.extend(VERSION.to_le_bytes()); // made by
            c.extend(VERSION.to_le_bytes()); // needed
            c.extend(UTF8.to_le_bytes());
            c.extend(0u16.to_le_bytes());
            c.extend(e.time.to_le_bytes());
            c.extend(e.date.to_le_bytes());
            c.extend(e.crc.to_le_bytes());
            c.extend(e.size.to_le_bytes());
            c.extend(e.size.to_le_bytes());
            c.extend((e.name.len() as u16).to_le_bytes());
            c.extend(0u16.to_le_bytes()); // extra
            c.extend(0u16.to_le_bytes()); // comment
            c.extend(0u16.to_le_bytes()); // disk
            c.extend(0u16.to_le_bytes()); // internal attributes
            c.extend(0u32.to_le_bytes()); // external attributes
            c.extend(e.offset.to_le_bytes());
            c.extend(e.name.as_bytes());
            self.put(&c)?;
        }
        let size =
            u32::try_from(self.written - u64::from(start)).map_err(|_| too_big("the bundle"))?;
        let count = entries.len() as u16;
        let mut end = Vec::with_capacity(22);
        end.extend(END.to_le_bytes());
        end.extend(0u16.to_le_bytes());
        end.extend(0u16.to_le_bytes());
        end.extend(count.to_le_bytes());
        end.extend(count.to_le_bytes());
        end.extend(size.to_le_bytes());
        end.extend(start.to_le_bytes());
        end.extend(0u16.to_le_bytes()); // comment length
        self.put(&end)?;
        self.out.flush()?;
        Ok(self.out)
    }
}

/// MS-DOS time and date in UTC; times before 1980 become 1980-01-01.
fn dos_time(t: SystemTime) -> (u16, u16) {
    let (y, m, d, hh, mm, ss) = uca_log::time::utc_fields(t);
    if y < 1980 {
        return (0, (1 << 5) | 1);
    }
    let y = u16::try_from((y - 1980).min(127)).unwrap_or(127);
    let time = ((hh as u16) << 11) | ((mm as u16) << 5) | (ss as u16 / 2);
    let date = (y << 9) | ((m as u16) << 5) | d as u16;
    (time, date)
}

/// CRC-32 (IEEE), as zip needs.
pub fn crc32(data: &[u8]) -> u32 {
    static TABLE: std::sync::OnceLock<[u32; 256]> = std::sync::OnceLock::new();
    let table = TABLE.get_or_init(|| {
        let mut t = [0u32; 256];
        for (i, slot) in t.iter_mut().enumerate() {
            let mut c = i as u32;
            for _ in 0..8 {
                c = if c & 1 != 0 {
                    0xEDB8_8320 ^ (c >> 1)
                } else {
                    c >> 1
                };
            }
            *slot = c;
        }
        t
    });
    let mut crc = !0u32;
    for &b in data {
        crc = table[((crc ^ u32::from(b)) & 0xff) as usize] ^ (crc >> 8);
    }
    !crc
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, UNIX_EPOCH};

    #[test]
    fn crc_check_value() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        assert_eq!(crc32(b""), 0);
    }

    fn u16_at(b: &[u8], i: usize) -> u16 {
        u16::from_le_bytes([b[i], b[i + 1]])
    }

    fn u32_at(b: &[u8], i: usize) -> u32 {
        u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]])
    }

    /// Reads the archive back through its central directory, as unzip does.
    #[test]
    fn entries_read_back_through_the_central_directory() {
        let t = UNIX_EPOCH + Duration::from_secs(1_791_755_403); // 2026-10-11 21:50:03
        let mut z = ZipWriter::new(Vec::new());
        z.add("logs/uca.2026-10-11.log", b"{\"msg\":\"hi\"}\n", t)
            .unwrap();
        z.add("README.txt", "héllo".as_bytes(), t).unwrap();
        let b = z.finish().unwrap();

        let end = b.len() - 22;
        assert_eq!(u32_at(&b, end), END);
        assert_eq!(u16_at(&b, end + 10), 2);
        let mut at = u32_at(&b, end + 16) as usize;
        let mut found = Vec::new();
        for _ in 0..2 {
            assert_eq!(u32_at(&b, at), CENTRAL);
            let crc = u32_at(&b, at + 16);
            let size = u32_at(&b, at + 20) as usize;
            let name_len = u16_at(&b, at + 28) as usize;
            let local = u32_at(&b, at + 42) as usize;
            let name = String::from_utf8(b[at + 46..at + 46 + name_len].to_vec()).unwrap();
            assert_eq!(u32_at(&b, local), LOCAL);
            let local_name_len = u16_at(&b, local + 26) as usize;
            let data = &b[local + 30 + local_name_len..local + 30 + local_name_len + size];
            assert_eq!(crc32(data), crc);
            assert_eq!(u16_at(&b, at + 8) & UTF8, UTF8);
            found.push((name, data.to_vec()));
            at += 46 + name_len;
        }
        assert_eq!(found[0].0, "logs/uca.2026-10-11.log");
        assert_eq!(
            found[1],
            ("README.txt".to_string(), "héllo".as_bytes().to_vec())
        );
        let (time, date) = dos_time(t);
        assert_eq!(date, ((2026 - 1980) << 9) | (10 << 5) | 11);
        assert_eq!(time, (21 << 11) | (50 << 5) | 1);
    }
}
