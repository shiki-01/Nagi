//! 依存なしの最小の PNG エンコーダー（RGB 8bit・無圧縮の deflate）。
//! スクリーンショットは AI による目視確認用で、サイズより依存の少なさを優先する。

fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &b in data {
        crc ^= u32::from(b);
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for &x in data {
        a = (a + u32::from(x)) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}

fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    let mut body = kind.to_vec();
    body.extend_from_slice(data);
    out.extend_from_slice(&body);
    out.extend_from_slice(&crc32(&body).to_be_bytes());
}

/// RGB（1 画素 3 バイト、上の行から）→ PNG
pub fn encode_rgb(width: u32, height: u32, rgb: &[u8]) -> Result<Vec<u8>, String> {
    let row = width as usize * 3;
    if rgb.len() != row * height as usize {
        return Err("画素数が幅×高さと一致しません".to_owned());
    }
    // 各行の先頭にフィルタ種別 0（なし）
    let mut raw = Vec::with_capacity((row + 1) * height as usize);
    for line in rgb.chunks(row.max(1)) {
        raw.push(0);
        raw.extend_from_slice(line);
    }
    // zlib: ヘッダ + 無圧縮ブロック（1 ブロック最大 65535 バイト）+ Adler-32
    let mut z = vec![0x78, 0x01];
    let mut blocks = raw.chunks(65535).peekable();
    if blocks.peek().is_none() {
        z.extend_from_slice(&[1, 0, 0, 0xFF, 0xFF]);
    }
    while let Some(block) = blocks.next() {
        z.push(u8::from(blocks.peek().is_none()));
        z.extend_from_slice(&(block.len() as u16).to_le_bytes());
        z.extend_from_slice(&(!(block.len() as u16)).to_le_bytes());
        z.extend_from_slice(block);
    }
    z.extend_from_slice(&adler32(&raw).to_be_bytes());

    let mut out = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.extend_from_slice(&[8, 2, 0, 0, 0]);
    chunk(&mut out, b"IHDR", &ihdr);
    chunk(&mut out, b"IDAT", &z);
    chunk(&mut out, b"IEND", &[]);
    Ok(out)
}

/// GDI の 32bit BGRA（BGRX）→ RGB
pub fn bgra_to_rgb(bgra: &[u8]) -> Vec<u8> {
    bgra.chunks_exact(4)
        .flat_map(|p| [p[2], p[1], p[0]])
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checksums_match_known_values() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        assert_eq!(adler32(b"Wikipedia"), 0x11E6_0398);
    }

    #[test]
    fn encodes_a_valid_structure() {
        let png = encode_rgb(2, 1, &[255, 0, 0, 0, 255, 0]).unwrap();
        assert_eq!(&png[..8], &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]);
        assert_eq!(&png[12..16], b"IHDR");
        assert!(png.ends_with(&[0, 0, 0, 0, b'I', b'E', b'N', b'D', 0xAE, 0x42, 0x60, 0x82]));
        assert!(encode_rgb(2, 2, &[0; 3]).is_err());
    }

    #[test]
    fn converts_bgra_to_rgb() {
        assert_eq!(
            bgra_to_rgb(&[1, 2, 3, 0, 4, 5, 6, 0]),
            vec![3, 2, 1, 6, 5, 4]
        );
    }
}
