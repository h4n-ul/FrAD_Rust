/**                              FrAD Profile 0                               */
/**
 * Copyright 2024 HaמuL
 * Function: FrAD Profile 0 encoding and decoding core
 */

pub mod backend;
use backend::{u8pack, core_fast::{dct, idct}};
pub mod profiles;

// Bit depth table
pub const DEPTHS: [i16; 6] = [12, 16, 24, 32, 48, 64];
// Dynamic ranges for preventing overflow
const FLOAT_DR: [i16; 6] = [5, 5, 8, 8, 11, 11];

/** analogue
 * Encodes PCM to FrAD
 * Parameters: f64 PCM, Bit depth, Little endian toggle (and possibly channel count, but it can be extracted from the PCM shape)
 * Returns: Encoded audio data, Encoded bit depth index, Encoded channel count
 */
pub fn analogue(pcm: Vec<Vec<f64>>, bits: i16, little_endian: bool) -> (Vec<u8>, i16, i16) {
    let pcm_trans: Vec<Vec<f64>> = (0..pcm[0].len())
        .map(|i| pcm.iter().map(|inner| inner[i]).collect())
        .collect();

    let freqs: Vec<Vec<f64>> = pcm_trans.iter().map(|x| dct(x.to_vec())).collect();
    let channels = freqs.len();

    let freqs_trans: Vec<Vec<f64>> = (0..freqs[0].len())
    .map(|i| freqs.iter().map(|inner| inner[i]).collect())
    .collect();

    let freqs_flat: Vec<f64> = freqs_trans.into_iter().flatten().collect();
    let mut bx = DEPTHS.iter().position(|&x| x == bits).unwrap();
    while freqs_flat.iter().max_by(|x, y| x.abs().partial_cmp(&y.abs()).unwrap()).unwrap().abs() > 2.0f64.powi(2.0f64.powi(FLOAT_DR[bx] as i32 - 1) as i32) {
        if bx == DEPTHS.len() { panic!("Overflow with reaching the max bit depth."); }
        bx += 1;
    }

    let frad = u8pack::pack(freqs_flat, bits, !little_endian);

    return (frad, DEPTHS.iter().position(|&x| x == bits).unwrap() as i16, channels as i16);
}

/** digital
 * Decodes FrAD to PCM
 * Parameters: Encoded audio data, Bit depth index, Channel count, Little endian toggle
 * Returns: Decoded PCM
 */
pub fn digital(frad: Vec<u8>, bits: i16, channels: i16, little_endian: bool) -> Vec<Vec<f64>> {
    let freqs_flat: Vec<f64> = u8pack::unpack(frad, DEPTHS[bits as usize], !little_endian);
    let channels = channels as usize;

    let samples = freqs_flat.len() / channels as usize;
    let freqs_trans: Vec<Vec<f64>> = (0..samples)
        .map(|i| freqs_flat[i * channels..(i + 1) * channels].to_vec())
        .collect();

    let freqs: Vec<Vec<f64>> = (0..freqs_trans[0].len())
        .map(|i| freqs_trans.iter().map(|inner| inner[i]).collect())
        .collect();

    let pcm_trans: Vec<Vec<f64>> = freqs.iter().map(|x| idct(x.to_vec())).collect();

    let pcm: Vec<Vec<f64>> = (0..pcm_trans[0].len())
        .map(|i| pcm_trans.iter().map(|inner| inner[i]).collect())
        .collect();
    return pcm;
}