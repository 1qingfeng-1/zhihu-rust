//! Request-body encryption and HMAC used by Zhihu's Android phone-login flow.
//!
//! The protocol is a table-driven block cipher: bytes go through a pair-swap
//! transform, CBC-like chaining and ten substitution rounds. The lookup tables
//! are copied verbatim from the reference client (see `docs/REWRITE_PLAN.md`).

use std::sync::OnceLock;

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use hmac::{Hmac, Mac};
use sha1::Sha1;

const BLOCK_SIZE: usize = 16;
const ROUNDS: usize = 10;
const PRE_TRANSFORM_MASK: u8 = 0xbb;
const IV: &[u8; 16] = b"f0551856aa575faa";

type HmacSha1 = Hmac<Sha1>;

pub fn hmac_sha1_hex(key: &str, data: &str) -> String {
    let mut mac = HmacSha1::new_from_slice(key.as_bytes()).expect("HMAC accepts any key length");
    mac.update(data.as_bytes());
    to_hex(&mac.finalize().into_bytes())
}

fn to_hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

struct Tables {
    round_keys: Vec<u8>,
    x0: Vec<u8>,
    xr: Vec<u8>,
    xf: Vec<u8>,
    t0: Vec<u8>,
    t1: Vec<u8>,
    t2: Vec<u8>,
    t3: Vec<u8>,
    sbox: Vec<u8>,
}

fn tables() -> &'static Tables {
    static TABLES: OnceLock<Tables> = OnceLock::new();
    TABLES.get_or_init(|| {
        let data = BASE64
            .decode(PROTOCOL_DATA)
            .expect("protocol table is valid base64");
        assert_eq!(data.len(), 5296, "unexpected protocol table size");
        Tables {
            round_keys: data[0..176].to_vec(),
            x0: data[176..432].to_vec(),
            xr: data[432..688].to_vec(),
            xf: data[688..944].to_vec(),
            t0: data[944..1968].to_vec(),
            t1: data[1968..2992].to_vec(),
            t2: data[2992..4016].to_vec(),
            t3: data[4016..5040].to_vec(),
            sbox: data[5040..5296].to_vec(),
        }
    })
}

/// Encrypt a form-encoded body exactly like the Android client does.
pub fn encrypt_body(form: &str) -> String {
    let tables = tables();
    let plain = form.as_bytes();
    let padding = BLOCK_SIZE - plain.len() % BLOCK_SIZE;
    let mut input = vec![0u8; plain.len() + padding];
    for (slot, byte) in input.iter_mut().zip(plain) {
        *slot = swap_pairs(*byte) ^ PRE_TRANSFORM_MASK;
    }
    let encoded_padding = swap_pairs(padding as u8) ^ PRE_TRANSFORM_MASK;
    input[plain.len()..].fill(encoded_padding);

    let mut previous: Vec<u8> = IV.iter().map(|byte| swap_pairs(*byte)).collect();
    let mut encrypted = vec![0u8; input.len()];
    for offset in (0..input.len()).step_by(BLOCK_SIZE) {
        let block: Vec<u8> = (0..BLOCK_SIZE)
            .map(|index| input[offset + index] ^ previous[index])
            .collect();
        previous = encrypt_block(tables, &block);
        for index in 0..BLOCK_SIZE {
            encrypted[offset + index] = swap_pairs(previous[index]);
        }
    }
    BASE64.encode(encrypted)
}

fn encrypt_block(tables: &Tables, block: &[u8]) -> Vec<u8> {
    let mut state = [0u8; BLOCK_SIZE];
    for index in 0..BLOCK_SIZE {
        state[index] = encoded_xor(&tables.x0, block[index], tables.round_keys[index]);
    }

    let positions = [[0, 4, 8, 12], [5, 9, 13, 1], [10, 14, 2, 6], [15, 3, 7, 11]];
    let matrix = [
        tables.t0.as_slice(),
        tables.t1.as_slice(),
        tables.t2.as_slice(),
        tables.t3.as_slice(),
    ];
    for round in 1..ROUNDS {
        let mut mixed = [0u8; BLOCK_SIZE];
        for (table_index, source_positions) in positions.iter().enumerate() {
            let table = matrix[table_index];
            for (word_index, source_position) in source_positions.iter().enumerate() {
                let table_offset = (state[*source_position] as usize) * 4;
                for byte_index in 0..4 {
                    let output_index = word_index * 4 + byte_index;
                    let value = table[table_offset + 3 - byte_index];
                    mixed[output_index] = if table_index == 0 {
                        value
                    } else {
                        encoded_xor(&tables.xr, mixed[output_index], value)
                    };
                }
            }
        }
        let key_offset = round * BLOCK_SIZE;
        for index in 0..BLOCK_SIZE {
            state[index] = encoded_xor(
                &tables.xr,
                mixed[index],
                tables.round_keys[key_offset + index],
            );
        }
    }

    let shifted_positions = [0, 5, 10, 15, 4, 9, 14, 3, 8, 13, 2, 7, 12, 1, 6, 11];
    let key_offset = ROUNDS * BLOCK_SIZE;
    let mut output = vec![0u8; BLOCK_SIZE];
    for index in 0..BLOCK_SIZE {
        let substituted = tables.sbox[state[shifted_positions[index]] as usize];
        output[index] = encoded_xor(
            &tables.xf,
            substituted,
            tables.round_keys[key_offset + index],
        );
    }
    output
}

fn encoded_xor(table: &[u8], left: u8, right: u8) -> u8 {
    let a = left as usize;
    let b = right as usize;
    let high = table[((a >> 4) << 4) ^ (b >> 4)] & 0xf0;
    let low = table[((a & 0x0f) << 4) ^ (b & 0x0f)] >> 4;
    high ^ low
}

fn swap_pairs(value: u8) -> u8 {
    ((value & 0x55) << 1) | ((value & 0xaa) >> 1)
}

// Protocol lookup data verified against the official Android 11.3.0 request-body vector.
const PROTOCOL_DATA: &str = concat!(
    "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",
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hmac_sha1_matches_rfc2202() {
        assert_eq!(
            hmac_sha1_hex("Jefe", "what do ya want for nothing?"),
            "effcdf6ae5eb2fa2d27416d5f184df9c259a7c79"
        );
    }

    #[test]
    fn body_encryption_is_block_aligned_and_deterministic() {
        let encrypted = encrypt_body("username=%2B8613800138000");
        let raw = BASE64.decode(&encrypted).unwrap();
        assert_eq!(raw.len() % BLOCK_SIZE, 0);
        assert!(raw.len() >= 32);
        assert_eq!(encrypted, encrypt_body("username=%2B8613800138000"));
        assert_ne!(encrypted, encrypt_body("username=%2B8613800138001"));
    }
}
