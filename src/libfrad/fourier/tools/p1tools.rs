/**                              Profile 1 Tools                              */
/**
 * Copyright 2024 HaמuL
 * Description: Quantisation and Dequantisation tools for Profile 1
 */

use crate::backend::{bitcvt, linspace};
use std::iter::repeat;

pub const SPREAD_ALPHA: f64 = 0.8;
const QUANT_ALPHA: f64 = 0.75;
pub const MOSLEN: usize = MODIFIED_OPUS_SUBBANDS.len() - 1;
const MODIFIED_OPUS_SUBBANDS: [u32; 28] = [
    0,     200,   400,   600,   800,   1000,  1200,  1400,
    1600,  2000,  2400,  2800,  3200,  4000,  4800,  5600,
    6800,  8000,  9600,  12000, 15600, 20000, 24000, 28800,
    34400, 40800, 48000, u32::MAX
];

/** get_bin_range
 * Gets the range of bins for a subband
 * Parameters: Length of the DCT Array, Signal sample rate, Subband index
 * Returns: Range of bins
 */
fn get_bin_range(len: usize, srate: u32, i: usize) -> std::ops::Range<usize> {
    let start = (MODIFIED_OPUS_SUBBANDS[i] as f64 / (srate as f64 / 2.0) * len as f64).round() as usize;
    let end = (MODIFIED_OPUS_SUBBANDS[i + 1] as f64 / (srate as f64 / 2.0) * len as f64).round() as usize;
    return start.min(len)..end.min(len);
}

/** mask_thres_mos
 * Calculates the masking threshold for each subband
 * Parameters: RMS of the subbands, Spread alpha(Constant for now)
 * Returns: Masking threshold array
 */
pub fn mask_thres_mos(mapped_freqs: &[f64], alpha: f64) -> Vec<f64> {
    let mut thres = vec![0.0; MOSLEN];

    // for each subband
    for i in 0..MOSLEN {
        // Centre frequency of the subband
        let f = (MODIFIED_OPUS_SUBBANDS[i] as f64 + MODIFIED_OPUS_SUBBANDS[i + 1] as f64) / 2.0;
        // Absolute Threshold of Hearing(in dB SPL)
        let abs = (3.64 * (f / 1000.0).powf(-0.8) - 6.5 * (-0.6 * (f / 1000.0 - 3.3).powi(2)).exp() + 1e-3 * (f / 1000.0).powi(4)).min(96.0);
        // Larger value between mapped_freq[i]^alpha and ATH in absolute amplitude
        thres[i] = mapped_freqs[i].powf(alpha).max(10.0_f64.powf((abs - 96.0) / 20.0));
    }

    return thres;
}

/** mapping_to_opus
 * Maps the frequencies to the modified Opus subbands
 * Parameters: DCT Array, Sample rate
 * Returns: Root mean square of the subbands
 */
pub fn mapping_to_opus(freqs: &[f64], srate: u32) -> Vec<f64> {
    let mut mapped_freqs = [0.0; MOSLEN].to_vec();

    for i in 0..MOSLEN {
        let subfreqs = freqs[get_bin_range(freqs.len(), srate, i)].to_vec();
        if !subfreqs.is_empty() {
            // Root mean square
            let sfq: f64 = subfreqs.iter().map(|x| x.powi(2)).sum::<f64>() / subfreqs.len() as f64;
            mapped_freqs[i] = sfq.sqrt();
        }
    }

    return mapped_freqs;
}

/** mapping_from_opus
 * Maps the frequencies from the modified Opus subbands
 * Parameters: MOS-Mapped frequencies, Length of the DCT Array, Sample rate
 * Returns: Inverse-mapped frequencies
 */
pub fn mapping_from_opus(mapped_freqs: &[f64], freqs_len: usize, srate: u32) -> Vec<f64> {
    let mut freqs = vec![0.0; freqs_len];

    for i in 0..MOSLEN-1 {
        let range = get_bin_range(freqs_len, srate, i);
        // Linearly spaced values between the mapped frequencies
        freqs[range.clone()].copy_from_slice(&linspace(mapped_freqs[i], mapped_freqs[i + 1], range.end - range.start));
    }

    return freqs;
}

/** quant
 * Non-linear quantisation function
 * Parameters: f64 value to quantise
 * Returns: Quantised value
 */
pub fn quant(x: f64) -> f64 { return x.signum() * x.abs().powf(QUANT_ALPHA); }

/** dequant
 * Non-linear dequantisation function
 * Parameters: f64 value to dequantise
 * Returns: Dequantised value
 */
pub fn dequant(y: f64) -> f64 { return y.signum() * y.abs().powf(1.0 / QUANT_ALPHA); }

/** exp_golomb_encode
 * Encodes any integer array with Exponential Golomb Encoding
 * Parameters: Integer array
 * Returns: Encoded binary data
 */
pub fn exp_golomb_encode(data: Vec<i64>) -> Vec<u8> {
    if data.is_empty() { return vec![0]; }
    let dmax = data.iter().map(|x| x.abs()).max().unwrap();
    let k = if dmax > 0 { (dmax as f64).log2().ceil() as u8 } else { 0 };

    let mut encoded_binary: Vec<bool> = bitcvt::to_bits(vec![k]);

    for n in data {
        let x = if n > 0 { (n << 1) - 1 } else { -n << 1 } + (1 << k);
        let code: Vec<bool> = bitcvt::to_bits(x.to_be_bytes().to_vec()).iter().skip_while(|&x| !x).cloned().collect();
        encoded_binary.extend(repeat(false).take(code.len() - (k + 1) as usize));
        encoded_binary.extend(code);
    }
    return bitcvt::to_bytes(encoded_binary);
}

/** exp_golomb_decode
 * Decodes any integer array with Exponential Golomb Encoding
 * Parameters: Binary data
 * Returns: Decoded integer array
 */
pub fn exp_golomb_decode(data: Vec<u8>) -> Vec<i64> {
    let k = data[0] as usize;
    let (data, kx, mut cache, mut idx) =
        (bitcvt::to_bits(data[1..].to_vec()), 1 << k, Vec::new(), 0);

    while idx < data.len() {
        let m = data[idx..].iter().position(|&x| x).unwrap_or(data.len());
        if m == data.len() { break; }
        let cwlen = (m * 2) + k + 1;
        cache.push(&data[idx+m..idx+cwlen.min(data.len())]);
        idx += cwlen;
    }

    return cache.into_iter().map(|codeword| {
        let n = codeword.iter().fold(0_i64, |acc, &bit| { (acc << 1) | (bit as i64) }) - kx;
        if n & 1 == 1 { (n + 1) >> 1 } else { -(n >> 1) }
    }).collect();
}