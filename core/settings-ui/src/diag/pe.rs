//! PE ファイル（exe・DLL）の CPU 種別の読み取り。配布物が OS の CPU と合っているかの判定に使う。

use std::io::Read;
use std::path::Path;

/// PE ヘッダーの `Machine`
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Machine {
    /// x64（`0x8664`）
    X64,
    /// ARM64（`0xAA64`）
    Arm64,
    /// x86（`0x014C`）
    X86,
    /// その他
    Other(u16),
}

impl Machine {
    /// 表示名
    pub fn label(self) -> String {
        match self {
            Machine::X64 => "x64".to_owned(),
            Machine::Arm64 => "ARM64".to_owned(),
            Machine::X86 => "x86".to_owned(),
            Machine::Other(m) => format!("不明(0x{m:04X})"),
        }
    }
}

/// 先頭のバイト列から `Machine` を読む（`MZ` → `e_lfanew` → `PE\0\0` → `Machine`）
pub fn parse_machine(bytes: &[u8]) -> Option<Machine> {
    if bytes.get(..2)? != b"MZ" {
        return None;
    }
    let offset = u32::from_le_bytes(bytes.get(0x3C..0x40)?.try_into().ok()?) as usize;
    if bytes.get(offset..offset + 4)? != b"PE\0\0" {
        return None;
    }
    let raw = u16::from_le_bytes(bytes.get(offset + 4..offset + 6)?.try_into().ok()?);
    Some(match raw {
        0x8664 => Machine::X64,
        0xAA64 => Machine::Arm64,
        0x014C => Machine::X86,
        other => Machine::Other(other),
    })
}

/// ファイルの `Machine` を読む（PE でなければ `Err`）
pub fn read_machine(path: &Path) -> Result<Machine, String> {
    let mut file = std::fs::File::open(path).map_err(|e| format!("開けません（{}）", e.kind()))?;
    let mut head = vec![0u8; 4096];
    let n = file
        .read(&mut head)
        .map_err(|e| format!("読めません（{}）", e.kind()))?;
    head.truncate(n);
    parse_machine(&head).ok_or_else(|| "PE ファイルではありません".to_owned())
}

#[cfg(test)]
pub(crate) fn fake_pe(machine: u16) -> Vec<u8> {
    let mut b = vec![0u8; 0x100];
    b[0] = b'M';
    b[1] = b'Z';
    b[0x3C..0x40].copy_from_slice(&0x80u32.to_le_bytes());
    b[0x80..0x84].copy_from_slice(b"PE\0\0");
    b[0x84..0x86].copy_from_slice(&machine.to_le_bytes());
    b
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_machine_types() {
        assert_eq!(parse_machine(&fake_pe(0x8664)), Some(Machine::X64));
        assert_eq!(parse_machine(&fake_pe(0xAA64)), Some(Machine::Arm64));
        assert_eq!(parse_machine(&fake_pe(0x014C)), Some(Machine::X86));
        assert_eq!(
            parse_machine(&fake_pe(0x1234)),
            Some(Machine::Other(0x1234))
        );
    }

    #[test]
    fn rejects_non_pe() {
        assert_eq!(parse_machine(b""), None);
        assert_eq!(parse_machine(b"MZ"), None);
        assert_eq!(parse_machine(&[0u8; 200]), None);
        let mut b = fake_pe(0x8664);
        b[0x80] = b'X';
        assert_eq!(parse_machine(&b), None);
        let mut b = fake_pe(0x8664);
        b[0x3C..0x40].copy_from_slice(&0xFFFF_FF00u32.to_le_bytes());
        assert_eq!(parse_machine(&b), None);
    }

    #[test]
    fn read_machine_from_file() {
        let dir = std::env::temp_dir().join(format!("nagi-diag-pe-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("a.dll");
        std::fs::write(&p, fake_pe(0xAA64)).unwrap();
        assert_eq!(read_machine(&p), Ok(Machine::Arm64));
        assert!(read_machine(&dir.join("none.dll")).is_err());
        std::fs::write(&p, b"text").unwrap();
        assert!(read_machine(&p).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
