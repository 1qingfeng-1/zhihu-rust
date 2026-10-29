//! Zhihu web request signing (`x-zse-93` / `x-zse-96`).
//!
//! Ported from Zhihu++ (`ZseSigner.kt` / `ZhihuFetchSignature.kt`), which is
//! AGPL-3.0. Keep the algorithm bit-for-bit identical to the reference.
//!
//! `x-zse-96 = "2.0_" + encrypt_zse_v4(md5_hex(zse93 + "+" + pathname + "+" + dc0 [+ "+" + body]))`

use md5::{Digest, Md5};

pub const ZHIHU_WEB_ZSE93: &str = "101_3_3.0";

const ZK: [u32; 32] = [
    1170614578, 1024848638, 1413669199, 3951632832, 3528873006, 2921909214, 4151847688, 3997739139,
    1933479194, 3323781115, 3888513386, 460404854, 3747539722, 2403641034, 2615871395, 2119585428,
    2265697227, 2035090028, 2773447226, 4289380121, 4217216195, 2200601443, 3051914490, 1579901135,
    1321810770, 456816404, 2903323407, 4065664991, 330002838, 3506006750, 363569021, 2347096187,
];

const ZB: [u8; 256] = [
    20, 223, 245, 7, 248, 2, 194, 209, 87, 6, 227, 253, 240, 128, 222, 91, 237, 9, 125, 157, 230,
    93, 252, 205, 90, 79, 144, 199, 159, 197, 186, 167, 39, 37, 156, 198, 38, 42, 43, 168, 217,
    153, 15, 103, 80, 189, 71, 191, 97, 84, 247, 95, 36, 69, 14, 35, 12, 171, 28, 114, 178, 148,
    86, 182, 32, 83, 158, 109, 22, 255, 94, 238, 151, 85, 77, 124, 254, 18, 4, 26, 123, 176, 232,
    193, 131, 172, 143, 142, 150, 30, 10, 146, 162, 62, 224, 218, 196, 229, 1, 192, 213, 27, 110,
    56, 231, 180, 138, 107, 242, 187, 54, 120, 19, 44, 117, 228, 215, 203, 53, 239, 251, 127, 81,
    11, 133, 96, 204, 132, 41, 115, 73, 55, 249, 147, 102, 48, 122, 145, 106, 118, 74, 190, 29, 16,
    174, 5, 177, 129, 63, 113, 99, 31, 161, 76, 246, 34, 211, 13, 60, 68, 207, 160, 65, 111, 82,
    165, 67, 169, 225, 57, 112, 244, 155, 51, 236, 200, 233, 58, 61, 47, 100, 137, 185, 64, 17, 70,
    234, 163, 219, 108, 170, 166, 59, 149, 52, 105, 24, 212, 78, 173, 45, 0, 116, 226, 119, 136,
    206, 135, 175, 195, 25, 92, 121, 208, 126, 139, 3, 75, 141, 21, 130, 98, 241, 40, 154, 66, 184,
    49, 181, 46, 243, 88, 101, 183, 8, 23, 72, 188, 104, 179, 210, 134, 250, 201, 164, 89, 216,
    202, 220, 50, 221, 152, 140, 33, 235, 214,
];

const ALPHABET: &str = "6fpLRqJO8M/c3jnYxFkUVC4ZIG12SiH=5v0mXDazWBTsuw7QetbKdoPyAl+hN9rgE";
const KEY16: [u8; 16] = *b"059053f7d15e01d7";
/// First plaintext byte used by the web client (`ZseSigner.kt`).
const SEED: u8 = 210;

#[inline]
fn read_u32_be(b: &[u8], off: usize) -> u32 {
    u32::from_be_bytes([b[off], b[off + 1], b[off + 2], b[off + 3]])
}

#[inline]
fn write_u32_be(v: u32, out: &mut [u8], off: usize) {
    out[off..off + 4].copy_from_slice(&v.to_be_bytes());
}

#[inline]
fn g_transform(tt: u32) -> u32 {
    let te = tt.to_be_bytes();
    let ti = u32::from_be_bytes([
        ZB[te[0] as usize],
        ZB[te[1] as usize],
        ZB[te[2] as usize],
        ZB[te[3] as usize],
    ]);
    ti ^ ti.rotate_left(2) ^ ti.rotate_left(10) ^ ti.rotate_left(18) ^ ti.rotate_left(24)
}

fn r_block(input16: &[u8]) -> [u8; 16] {
    let mut tr = [0u32; 36];
    tr[0] = read_u32_be(input16, 0);
    tr[1] = read_u32_be(input16, 4);
    tr[2] = read_u32_be(input16, 8);
    tr[3] = read_u32_be(input16, 12);

    for i in 0..32 {
        let ta = g_transform(tr[i + 1] ^ tr[i + 2] ^ tr[i + 3] ^ ZK[i]);
        tr[i + 4] = tr[i] ^ ta;
    }

    let mut out = [0u8; 16];
    write_u32_be(tr[35], &mut out, 0);
    write_u32_be(tr[34], &mut out, 4);
    write_u32_be(tr[33], &mut out, 8);
    write_u32_be(tr[32], &mut out, 12);
    out
}

fn x_blocks(data: &[u8], mut iv: [u8; 16]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len());
    for chunk in data.chunks(16) {
        let mut mixed = [0u8; 16];
        for i in 0..16 {
            mixed[i] = chunk[i] ^ iv[i];
        }
        iv = r_block(&mixed);
        out.extend_from_slice(&iv);
    }
    out
}

fn custom_encode(mut bytes: Vec<u8>) -> String {
    while !bytes.len().is_multiple_of(3) {
        bytes.push(0);
    }

    let alphabet = ALPHABET.as_bytes();
    let mut out = String::with_capacity((bytes.len() / 3) * 4);
    let mut i: u32 = 0;
    let mut p = bytes.len() as isize - 1;

    while p >= 0 {
        let mut v: u32 = 0;

        let b0 = bytes[p as usize] as u32;
        let m0 = (58u32 >> (8 * (i % 4))) & 0xFF;
        i += 1;
        v |= (b0 ^ m0) & 0xFF;

        let b1 = bytes[(p - 1) as usize] as u32;
        let m1 = (58u32 >> (8 * (i % 4))) & 0xFF;
        i += 1;
        v |= ((b1 ^ m1) & 0xFF) << 8;

        let b2 = bytes[(p - 2) as usize] as u32;
        let m2 = (58u32 >> (8 * (i % 4))) & 0xFF;
        i += 1;
        v |= ((b2 ^ m2) & 0xFF) << 16;

        out.push(alphabet[(v & 63) as usize] as char);
        out.push(alphabet[((v >> 6) & 63) as usize] as char);
        out.push(alphabet[((v >> 12) & 63) as usize] as char);
        out.push(alphabet[((v >> 18) & 63) as usize] as char);

        p -= 3;
    }

    out
}

/// Encrypt a plaintext with Zhihu's `zse v4` variant.
pub fn encrypt_zse_v4(input: &str) -> String {
    let mut plain = Vec::with_capacity(input.len() + 32);
    plain.push(SEED);
    plain.push(0);
    plain.extend_from_slice(input.as_bytes());

    let pad = 16 - (plain.len() % 16);
    for _ in 0..pad {
        plain.push(pad as u8);
    }

    let mut first = [0u8; 16];
    for i in 0..16 {
        first[i] = plain[i] ^ KEY16[i] ^ 42;
    }

    let c0 = r_block(&first);
    let mut cipher = Vec::with_capacity(plain.len());
    cipher.extend_from_slice(&c0);
    if plain.len() > 16 {
        cipher.extend_from_slice(&x_blocks(&plain[16..], c0));
    }

    custom_encode(cipher)
}

pub fn md5_hex(input: &str) -> String {
    let mut hasher = Md5::new();
    hasher.update(input.as_bytes());
    let digest = hasher.finalize();
    let mut out = String::with_capacity(32);
    for byte in digest {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

/// Build the signed path component exactly like the reference:
/// `"/" + url.substringAfter("//").substringAfter('/')`.
pub fn pathname(url: &str) -> String {
    let after_scheme = url.split_once("//").map(|(_, rest)| rest).unwrap_or(url);
    let after_host = after_scheme
        .split_once('/')
        .map(|(_, rest)| rest)
        .unwrap_or("");
    format!("/{after_host}")
}

/// Build the `x-zse-96` header value.
pub fn zse96(zse93: &str, url: &str, dc0: &str, body: Option<&str>) -> String {
    let path = pathname(url);
    let mut source = format!("{zse93}+{path}+{dc0}");
    if let Some(body) = body {
        source.push('+');
        source.push_str(body);
    }
    format!("2.0_{}", encrypt_zse_v4(&md5_hex(&source)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn md5_matches_rfc() {
        assert_eq!(md5_hex(""), "d41d8cd98f00b204e9800998ecf8427e");
        assert_eq!(md5_hex("abc"), "900150983cd24fb0d6963f7d28e17f72");
    }

    #[test]
    fn pathname_keeps_query() {
        assert_eq!(
            pathname("https://www.zhihu.com/api/v4/me?x=1"),
            "/api/v4/me?x=1"
        );
        assert_eq!(pathname("https://www.zhihu.com/"), "/");
    }

    #[test]
    fn zse_header_shape() {
        let header = zse96(
            ZHIHU_WEB_ZSE93,
            "https://www.zhihu.com/api/v4/me",
            "dc0",
            None,
        );
        assert!(header.starts_with("2.0_"));
        assert!(header.len() > 20);
    }
}
