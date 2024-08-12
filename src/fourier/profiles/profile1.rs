/**                              FrAD Profile 1                               */
/**
 * Copyright 2024 HaמuL
 * Function: FrAD Profile 1 encoding and decoding core
 * Dependencies: flate2, half
 */

use super::{super::backend::core::{dct, idct}, tools::p1tools};

use flate2::{write::ZlibEncoder, read::ZlibDecoder, Compression};
use std::io::prelude::*;

// Sample rate table
pub const SRATES: [u32; 12] = [96000, 88200, 64000, 48000, 44100, 32000, 24000, 22050, 16000, 12000, 11025, 8000];
// Sample count table
pub const SMPLS: [(u32, [u32; 8]); 3] = [
    (128, [128, 256, 512, 1024, 2048, 4096, 8192, 16384]),
    (144, [144, 288, 576, 1152, 2304, 4608, 9216, 18432]),
    (192, [192, 384, 768, 1536, 3072, 6144, 12288, 24576]),
];

// Get sample count multiplier from value
pub fn get_smpls_from_value(key: &u32) -> u32 {
    return SMPLS.iter().find(|&(_, v)| v.iter().any(|&x| x == *key)).unwrap().0;
}

// Sample count list
pub const SMPLS_LI: [u32; 24] = smpls_li();
const fn smpls_li() -> [u32; 24] {
    let mut result = [0; 24];
    let (mut index, mut s) = (0, 0);
    while s < 8 {
        let mut i = 0;
        while i < 3 {
            result[index] = SMPLS[i].1[s];
            (index, i) = (index+1, i+1);
        } s += 1;
    } return result;
}

pub const MAX_SMPL: u32 = max_smpl();
const fn max_smpl() -> u32 {
    let (mut max, mut i) = (0, 0);
    while i < SMPLS_LI.len() {
        if SMPLS_LI[i] > max {
            max = SMPLS_LI[i];
        } i += 1;
    } return max;
}

// Bit depth table
pub const DEPTHS: [i16; 7] = [8, 12, 16, 24, 32, 48, 64];

/** pad_pcm
 * Pads the PCM to the nearest sample count greater than the original
 * Parameters: f64 PCM
 * Returns: Padded f64 PCM
 */
fn pad_pcm(mut pcm: Vec<Vec<f64>>) -> Vec<Vec<f64>> {
    let len_smpl = pcm.len();
    let chnl = pcm[0].len();
    let pad_len = *SMPLS_LI.iter().find(|&&x| x as usize >= len_smpl).unwrap_or(&(len_smpl as u32)) as usize - len_smpl;

    pcm.extend(std::iter::repeat(vec![0.0; chnl]).take(pad_len));
    return pcm;
}

/** analogue
 * Encodes PCM to FrAD Profile 1
 * Parameters: f64 PCM, Bit depth, Sample rate, Loss level (and channel count, same note as profile 0)
 * Returns: Encoded audio data, Encoded bit depth index, Encoded channel count
 */
pub fn analogue(pcm: Vec<Vec<f64>>, bits: i16, srate: u32, level: u8) -> (Vec<u8>, i16, i16) {
    let pcm = pad_pcm(pcm);
    let pcm_trans: Vec<Vec<f64>> = (0..pcm[0].len())
        .map(|i| pcm.iter().map(|inner| inner[i] * 2.0_f64.powf((bits-1) as f64)).collect())
        .collect();

    let freqs: Vec<Vec<f64>> = pcm_trans.iter().map(|x| dct(x.to_vec())).collect();
    let channels = freqs.len();

    let const_factor = 1.25_f64.powi(level as i32) / 19.0 + 0.5;

    // Subband masking and quantisation
    let mut subband_sgnl: Vec<Vec<i64>> = vec![vec![0; freqs[0].len()]; channels as usize];
    let mut thres: Vec<Vec<i64>> = vec![vec![0; p1tools::MOSLEN]; channels as usize];

    for c in 0..channels as usize {
        let absfreqs = freqs[c].iter().map(|x| x.abs()).collect::<Vec<f64>>();
        let mapping = p1tools::mapping_to_opus(&absfreqs, srate);
        let thres_channel: Vec<f64> = p1tools::mask_thres_mos(&mapping, p1tools::ALPHA).iter().map(|x| x * const_factor).collect();
        thres[c] = thres_channel.iter().map(|x| (x * 2.0_f64.powi(16 - bits as i32)).round() as i64).collect();

        let div_factor = p1tools::mapping_from_opus(&thres_channel, freqs[0].len(), srate);

        let masked: Vec<i64> = freqs[c].iter().zip(div_factor).map(|(x, y)| p1tools::quant(x / y).round() as i64).collect();
        subband_sgnl[c] = masked;
    }

    let freqs_flat: Vec<i64> = (0..subband_sgnl[0].len()).flat_map(|i| subband_sgnl.iter().map(move |inner| inner[i])).collect();
    let freqs_gol: Vec<u8> = p1tools::exp_golomb_rice_encode(freqs_flat);

    let thres_flat: Vec<i64> = (0..thres[0].len()).flat_map(|i| thres.iter().map(move |inner| inner[i])).collect();
    let thres_gol: Vec<u8> = p1tools::exp_golomb_rice_encode(thres_flat);

    let frad: Vec<u8> = (thres_gol.len() as u32).to_be_bytes().to_vec().into_iter().chain(thres_gol).chain(freqs_gol).collect();

    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::best());
    encoder.write_all(&frad).unwrap();
    let frad = encoder.finish().unwrap();

    return (frad, DEPTHS.iter().position(|&x| x == bits).unwrap() as i16, channels as i16);
}

/** digital
 * Decodes FrAD Profile 1 to PCM
 * Parameters: Encoded audio data, Bit depth index, Channel count, Sample rate(for dequantisation)
 * Returns: f64 PCM
 */
pub fn digital(frad: Vec<u8>, bits: i16, channels: i16, srate: u32) -> Vec<Vec<f64>> {
    let channels = channels as usize;

    let mut decoder = ZlibDecoder::new(&frad[..]);
    let frad = {
        let mut buf = Vec::new();
        decoder.read_to_end(&mut buf).unwrap();
        buf
    };

    let thres_len = u32::from_be_bytes(frad[0..4].try_into().unwrap()) as usize;

    let thres_gol = frad[4..4+thres_len].to_vec();
    let freqs_gol = frad[4+thres_len..].to_vec();

    let freqs_flat: Vec<f64> = p1tools::exp_golomb_rice_decode(freqs_gol).iter().map(|x| *x as f64).collect();
    let pns_flat: Vec<f64> = p1tools::exp_golomb_rice_decode(thres_gol).iter().map(|x| *x as f64 / 2.0_f64.powi(16 - bits as i32)).collect();

    let subband_sgnl: Vec<Vec<f64>> = (0..channels)
        .map(|i| freqs_flat.iter().skip(i).step_by(channels).copied().collect()).collect();
    let masks: Vec<Vec<f64>> = (0..channels)
        .map(|i| pns_flat.iter().skip(i).step_by(channels).copied().collect()).collect();

    let mut freqs: Vec<Vec<f64>> = vec![vec![0.0; subband_sgnl[0].len()]; channels as usize];

    for c in 0..channels as usize {
        freqs[c] = subband_sgnl[c].iter()
            .zip(p1tools::mapping_from_opus(&masks[c], subband_sgnl[c].len(), srate))
            .map(|(x, y)| p1tools::dequant(*x) * y)
            .collect();
    }

    let pcm_trans: Vec<Vec<f64>> = freqs.iter().map(|x| idct(x.to_vec())).collect();

    let pcm: Vec<Vec<f64>> = (0..pcm_trans[0].len())
        .map(|i| pcm_trans.iter().map(|inner| inner[i] / 2.0_f64.powi(bits as i32 - 1)).collect())
        .collect();
    return pcm;
}