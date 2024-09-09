/**                                  Decode                                   */
/**
 * Copyright 2024 HaמuL
 * Function: Decode any file containing FrAD frames to PCM
 */

use crate::{backend::{linspace, SplitFront, VecPatternFind}, common::{self, f64_to_any, PCMFormat},
    fourier::profiles::{profile0, profile1, profile4, COMPACT, LOSSLESS},
    tools::{asfh::ASFH, cli, ecc, log::LogObj}};
use std::{fs::File, io::{ErrorKind, Read, Write}, path::Path, process::exit};
use rodio::{buffer::SamplesBuffer, OutputStream, Sink};
use same_file::is_same_file;

/** write
 * Writes PCM data to file or sink
 * Parameters: Play flag, Output file/sink, PCM data, PCM format, Sample rate
 * Parameters: Output file, PCM data
 * Returns: None
 */
fn write(isplay: bool, file: &mut Box<dyn Write>, sink: &mut Sink, pcm: Vec<Vec<f64>>, fmt: &PCMFormat, srate: &u32) {
    if pcm.is_empty() { return; }
    if isplay {
        sink.append(SamplesBuffer::new(
            pcm[0].len() as u16,
            *srate,
            pcm.into_iter().flatten().map(|x| x as f32).collect::<Vec<f32>>()
        ));
    }
    else {
        let pcm_bytes: Vec<u8> = pcm.into_iter().flatten().flat_map(|x| f64_to_any(x, fmt)).collect();
        file.write_all(&pcm_bytes)
        .unwrap_or_else(|err|
            if err.kind() == ErrorKind::BrokenPipe { std::process::exit(0); } else { panic!("Error writing to stdout: {}", err); }
        );
    }
}

/** Decode
 * Struct for FrAD decoder
 */
pub struct Decode {
    asfh: ASFH, info: ASFH,
    buffer: Vec<u8>,
    overlap_fragment: Vec<Vec<f64>>,
    log: LogObj,

    fix_error: bool,
}

impl Decode {
    pub fn new(fix_error: bool, loglevel: u8) -> Decode {
        Decode {
            asfh: ASFH::new(), info: ASFH::new(),
            buffer: Vec::new(),
            overlap_fragment: Vec::new(),
            log: LogObj::new(loglevel, 0.5),

            fix_error,
        }
    }

    /** overlap
     * Apply overlap to the decoded PCM
     * Parameters: Decoded PCM
     * Returns: PCM with overlap applied
     */
    fn overlap(&mut self, mut frame: Vec<Vec<f64>>) -> Vec<Vec<f64>> {
        // 1. If overlap buffer not empty, apply Forward-linear overlap-add
        if !self.overlap_fragment.is_empty() {
            let fade_in: Vec<f64> = linspace(0.0, 1.0, self.overlap_fragment.len());
            let fade_out: Vec<f64> = linspace(1.0, 0.0, self.overlap_fragment.len());
            for c in 0..self.asfh.channels as usize {
                for i in 0..self.overlap_fragment.len() {
                    frame[i][c] = frame[i][c] * fade_in[i] + self.overlap_fragment[i][c] * fade_out[i];
                }
            }
        }

        // 2. if COMPACT profile and overlap is enabled, split this frame
        let mut next_overlap = Vec::new();
        if COMPACT.contains(&self.asfh.profile) && self.asfh.olap != 0 {
            let olap = self.asfh.olap.max(2);
            // return_frame         = frame[0 ~ (len*(olap-1)) / olap]
            // new_overlap_fragment = frame[(len*(olap-1)) / olap ~ len]
            // = [2048], olap=16 -> [1920, 128]
            next_overlap = frame.split_off((frame.len() * (olap as usize - 1)) / olap as usize);
        }
        self.overlap_fragment = next_overlap;
        return frame;
    }

    /** process
     * Process the input stream and decode the FrAD frames
     * Parameters: Input stream
     * Returns: Decoded PCM, Sample rate, Channels, Critical info modification flag
     */
    pub fn process(&mut self, stream: Vec<u8>) -> (Vec<Vec<f64>>, u32, bool) {
        self.buffer.extend(stream);
        let mut ret = Vec::new();

        loop {
            // If every parameter in the ASFH struct is set,
            /* 1. Decoding FrAD Frame */
            if self.asfh.all_set {
                // 1.0. If the buffer is not enough to decode the frame, break
                if self.buffer.len() < self.asfh.frmbytes as usize { break; }

                // 1.1. Split out the frame data
                let mut frad: Vec<u8> = self.buffer.split_front(self.asfh.frmbytes as usize);

                // 1.2. Correct the error if ECC is enabled
                if self.asfh.ecc {
                    if self.fix_error && ( // and if the user requested
                        // and if CRC mismatch
                        LOSSLESS.contains(&self.asfh.profile) && common::crc32(&frad) != self.asfh.crc32 ||
                        COMPACT.contains(&self.asfh.profile) && common::crc16_ansi(&frad) != self.asfh.crc16
                    ) { frad = ecc::decode_rs(frad, self.asfh.ecc_ratio[0] as usize, self.asfh.ecc_ratio[1] as usize); } // Error correction
                    else { frad = ecc::unecc(frad, self.asfh.ecc_ratio[0] as usize, self.asfh.ecc_ratio[1] as usize); } // ECC removal
                }

                // 1.3. Decode the FrAD frame
                let mut pcm =
                match self.asfh.profile {
                    1 => profile1::digital(frad, self.asfh.bit_depth, self.asfh.channels, self.asfh.srate),
                    4 => profile4::digital(frad, self.asfh.bit_depth, self.asfh.channels, self.asfh.endian),
                    _ => profile0::digital(frad, self.asfh.bit_depth, self.asfh.channels, self.asfh.endian)
                };

                // 1.4. Apply overlap
                pcm = self.overlap(pcm); let samples = pcm.len();
                self.log.update(&self.asfh.total_bytes, samples, &self.asfh.srate);
                self.log.logging(false);

                // 1.5. Append the decoded PCM and clear header
                ret.extend(pcm);
                self.asfh.clear();
            }

            /* 2. Finding header / Gathering more data to parse */
            else {
                // 2.1. If the header buffer not found, find the header buffer
                if !self.asfh.buffer.starts_with(&common::FRM_SIGN) {
                    match self.buffer.find_pattern(&common::FRM_SIGN) {
                        // If pattern found in the buffer
                        // 2.1.1. Split out the buffer to the header buffer
                        Some(i) => {
                            self.buffer.split_front(i);
                            self.asfh.buffer = self.buffer.split_front(4);
                        },
                        // 2.1.2. else, Split out the buffer to the last 4 bytes and return
                        None => {
                            self.buffer.split_front(self.buffer.len().saturating_sub(4)); break; 
                        }
                    }
                }
                // 2.2. If header buffer found, try parsing the header
                let force_flush = self.asfh.read_buf(&mut self.buffer);

                // 2.3. Check header parsing result
                match force_flush {
                    // 2.3.1. If header is complete and not forced to flush, continue
                    Ok(false) => {
                        // 2.3.1.1. If any critical parameter has changed, flush the overlap buffer
                        if !self.asfh.criteq(&self.info) {
                            if self.info.srate != 0 || self.info.channels != 0 { // If the info struct is not empty
                                ret.extend(self.flush()); // Flush the overlap buffer
                                let srate = self.info.srate; // Save the sample rate
                                self.info = self.asfh.clone(); // Update the info struct
                                return (ret, srate, true); // and return
                            }
                            self.info = self.asfh.clone(); // else, Update the info struct and continue
                        }
                    },
                    // 2.3.2. If header is complete and forced to flush, flush and return
                    Ok(true) => { ret.extend(self.flush()); break; },
                    // 2.3.3. If header is incomplete, return
                    Err(_) => break,
                }
            }
        }
        return (ret, self.asfh.srate, false);
    }

    /** flush
     * Flush the overlap buffer
     * Parameters: None
     * Returns: Overlap buffer
     */
    pub fn flush(&mut self) -> Vec<Vec<f64>> {
        // 1. Extract the overlap buffer
        // 2. Update log
        // 3. Clear the overlap buffer
        // 4. Clear the ASFH struct
        // 5. Return exctacted buffer

        let ret = self.overlap_fragment.clone();
        self.log.update(&0, self.overlap_fragment.len(), &self.asfh.srate);
        self.overlap_fragment.clear();
        self.asfh.clear();
        return ret;
    }
}

/** decode
 * Decodes any found FrAD frames in the input file to f64be PCM
 * Parameters: Input file, CLI parameters
 * Returns: Decoded PCM on File or stdout
 */
pub fn decode(rfile: String, params: cli::CliParams, mut loglevel: u8) {
    let mut wfile = params.output;
    if rfile.is_empty() { panic!("Input file must be given"); }

    let mut rpipe = false;
    if common::PIPEIN.contains(&rfile.as_str()) { rpipe = true; }
    else if !Path::new(&rfile).exists() { panic!("Input file does not exist"); }

    let mut wpipe = false;
    if common::PIPEOUT.contains(&wfile.as_str()) { wpipe = true; }
    else {
        match is_same_file(&rfile, &wfile) {
            Ok(true) => { eprintln!("Input and output files cannot be the same"); exit(1); }
            _ => {}
        }
        if wfile.is_empty() {
            let wfrf = Path::new(&rfile).file_name().unwrap().to_str().unwrap().to_string();
            wfile = wfrf.split(".").collect::<Vec<&str>>()[..wfrf.split(".").count() - 1].join(".");
        }
        else if wfile.ends_with(".pcm") { wfile = wfile[..wfile.len() - 4].to_string(); }

        if Path::new(&wfile).exists() && !params.overwrite {
            eprintln!("Output file already exists, overwrite? (Y/N)");
            loop {
                let mut input = String::new();
                std::io::stdin().read_line(&mut input).unwrap();
                if input.trim().to_lowercase() == "y" { break; }
                else if input.trim().to_lowercase() == "n" {
                    eprintln!("Aborted.");
                    std::process::exit(0);
                }
            }
        }
    }
    let play = params.play;
    let mut readfile: Box<dyn Read> = if !rpipe { Box::new(File::open(rfile).unwrap()) } else { Box::new(std::io::stdin()) };
    let mut writefile: Box<dyn Write> = if !wpipe && !play { Box::new(File::create(format!("{}.pcm", wfile)).unwrap()) } else { Box::new(std::io::stdout()) };
    let (_stream, stream_handle) = OutputStream::try_default().unwrap();
    let mut sink = Sink::try_new(&stream_handle).unwrap();
    sink.set_speed(params.speed as f32);

    if play { loglevel = 0; }
    let mut decoder = Decode::new(params.enable_ecc, loglevel);
    let pcm_fmt = params.pcm;

    let mut no = 0;
    loop {
        let mut buf = vec![0u8; 32768];
        let readlen = common::read_exact(&mut readfile, &mut buf);

        if readlen == 0 && decoder.buffer.is_empty() && (!play || sink.empty()) { break; }

        let (pcm, srate, critical_info_modified): (Vec<Vec<f64>>, u32, bool);
        (pcm, srate, critical_info_modified) = decoder.process(buf[..readlen].to_vec());
        write(play, &mut writefile, &mut sink, pcm, &pcm_fmt, &srate);

        if critical_info_modified && !(wpipe || play) {
            no += 1; writefile = Box::new(File::create(format!("{}.{}.pcm", wfile, no)).unwrap());
        }
    }
    write(play, &mut writefile, &mut sink, decoder.flush(), &pcm_fmt, &decoder.asfh.srate);

    decoder.log.logging(true);
    if play { sink.sleep_until_end(); }
}