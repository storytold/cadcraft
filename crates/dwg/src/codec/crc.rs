//! Checksums used by DWG files.
//!
//! Spec §2.14 "CRCS" and §4.2 "Section page checksum" (verified against the
//! spec text). The spec calls the first one an "8-bit CRC" because it walks
//! the data a byte at a time; the value itself is 16 bits. Its table is the
//! reflected 0xA001 polynomial table, which is generated here rather than
//! copied.

#[allow(clippy::indexing_slicing)]
const fn make_crc16_table() -> [u16; 256] {
    let mut table = [0u16; 256];
    let mut i = 0;
    while i < 256 {
        let mut c = i as u16;
        let mut k = 0;
        while k < 8 {
            c = if c & 1 != 0 { (c >> 1) ^ 0xA001 } else { c >> 1 };
            k += 1;
        }
        table[i] = c;
        i += 1;
    }
    table
}

#[allow(clippy::indexing_slicing)]
const fn make_crc32_table() -> [u32; 256] {
    let mut table = [0u32; 256];
    let mut i = 0;
    while i < 256 {
        let mut c = i as u32;
        let mut k = 0;
        while k < 8 {
            c = if c & 1 != 0 { (c >> 1) ^ 0xEDB8_8320 } else { c >> 1 };
            k += 1;
        }
        table[i] = c;
        i += 1;
    }
    table
}

// The tables have 256 entries and are indexed by a byte: indexing them can't fail.
static CRC16_TABLE: [u16; 256] = make_crc16_table();
static CRC32_TABLE: [u32; 256] = make_crc32_table();

/// The DWG "8-bit CRC" (16-bit result). Seeds seen in the spec: 0xC0C1 for
/// sections and objects, 0 for the R13–R15 file header.
#[allow(clippy::indexing_slicing)]
pub fn crc16(seed: u16, data: &[u8]) -> u16 {
    let mut crc = seed;
    for &b in data {
        crc = (crc >> 8) ^ CRC16_TABLE[((crc ^ b as u16) & 0xFF) as usize];
    }
    crc
}

/// The 32-bit CRC used from R2004 (R18) on. Standard reflected CRC-32 with
/// a caller-supplied seed; seed 0 gives the common CRC-32 value.
#[allow(clippy::indexing_slicing)]
pub fn crc32(seed: u32, data: &[u8]) -> u32 {
    let mut crc = !seed;
    for &b in data {
        crc = (crc >> 8) ^ CRC32_TABLE[((crc ^ b as u32) & 0xFF) as usize];
    }
    !crc
}

/// Section page checksum (§4.2): an Adler-style running sum with a seed.
pub fn page_checksum(seed: u32, data: &[u8]) -> u32 {
    let mut sum1 = seed & 0xFFFF;
    let mut sum2 = seed >> 16;
    for chunk in data.chunks(0x15B0) {
        for &b in chunk {
            sum1 += b as u32;
            sum2 += sum1;
        }
        sum1 %= 0xFFF1;
        sum2 %= 0xFFF1;
    }
    (sum2 << 16) | (sum1 & 0xFFFF)
}

/// XOR applied to the R13–R15 file-header CRC, by number of section
/// locator records (§3.2.6).
pub fn file_header_crc_xor(records: u32) -> u16 {
    match records {
        3 => 0xA598,
        4 => 0x8101,
        5 => 0x3CC4,
        6 => 0x8461,
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc16_table_matches_spec_prefix() {
        // First entries as printed in §2.14.1.
        let expected: [u16; 16] =
            [0x0000, 0xC0C1, 0xC181, 0x0140, 0xC301, 0x03C0, 0x0280, 0xC241, 0xC601, 0x06C0, 0x0780, 0xC741, 0x0500, 0xC5C1, 0xC481, 0x0440];
        assert_eq!(&CRC16_TABLE[..16], &expected);
    }

    #[test]
    fn crc32_table_matches_spec_prefix() {
        assert_eq!(&CRC32_TABLE[..4], &[0x0000_0000, 0x7707_3096, 0xEE0E_612C, 0x9909_51BA]);
    }

    #[test]
    fn standard_check_values() {
        // CRC-16/ARC and CRC-32 check values for "123456789".
        assert_eq!(crc16(0, b"123456789"), 0xBB3D);
        assert_eq!(crc32(0, b"123456789"), 0xCBF4_3926);
    }

    #[test]
    fn page_checksum_matches_adler_with_seed_one() {
        // With seed 1 this is exactly Adler-32; "Wikipedia" -> 0x11E60398.
        assert_eq!(page_checksum(1, b"Wikipedia"), 0x11E6_0398);
    }
}
