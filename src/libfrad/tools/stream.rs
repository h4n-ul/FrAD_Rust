/**                                Stream Info                                */
/**
 * Copyright 2024 HaמuL
 * Description: Stream information container
 */

use std::{collections::HashMap, time::Instant};

/** StreamInfo
 * Struct for stream information
 */
pub struct StreamInfo {
    start_time: Instant,
    t_block: Option<Instant>,
    total_size: u128,
    duration: HashMap<u32, u128>,
    bitrate: HashMap<u32, u128>,
}

impl StreamInfo {
    pub fn new() -> StreamInfo {
        StreamInfo {
            start_time: Instant::now(),
            t_block: None,
            duration: HashMap::new(),
            total_size: 0,
            bitrate: HashMap::new(),
        }
    }

    /** update
     * Accumulates total stream size, duration within sample rate, and bitrate
     * Parameters: Stream size, Sample count, Sample rate
     */
    pub fn update(&mut self, size: &u128, samples: usize, srate: &u32) {
        self.total_size += size;
        self.duration.insert(*srate, if self.duration.contains_key(&srate) { self.duration[&srate] } else { 0 } + samples as u128);
        self.bitrate.insert(*srate, if self.bitrate.contains_key(&srate) { self.bitrate[&srate] } else { 0 } + *size as u128);
    }

    /** get_duration
     * Gets the total duration of the stream in f64 seconds
     * Returns: Total duration
     */
    pub fn get_duration(&self) -> f64 {
        return self.duration.iter().map(|(k, v)| *v as f64 / *k as f64).sum();
    }

    /** get_bitrate
     * Gets the total bitrate of the stream in f64 bits per second
     * Returns: Total bitrate
     */
    pub fn get_bitrate(&self) -> f64 {
        let total_bits: f64 = self.bitrate.values().sum::<u128>() as f64 * 8.0;
        let total_duration: f64 = self.duration.iter().map(|(k, v)| *v as f64 / *k as f64).sum();
        return if total_duration > 0.0 { total_bits / total_duration } else { 0.0 };
    }

    /** get_speed
     * Gets the coding speed of the stream in f64 samples per second
     * Returns: Coding speed
     */
    pub fn get_speed(&self) -> f64 {
        let encoding_time = self.start_time.elapsed().as_secs_f64();
        let total_duration: f64 = self.duration.iter().map(|(k, v)| *v as f64 / *k as f64).sum();
        return if encoding_time > 0.0 { total_duration / encoding_time } else { 0.0 };
    }

    /** get_total_size
     * Getter for private total_size
     * Returns: Total size
     */
    pub fn get_total_size(&self) -> u128 { return self.total_size; }

    /** block
     * Blocks the stream timer
     */
    pub fn block(&mut self) {
        self.t_block = Some(Instant::now());
    }

    /** unblock
     * Unblocks the stream timer
     */
    pub fn unblock(&mut self) {
        if let Some(t_block) = self.t_block {
            self.start_time += t_block.elapsed();
            self.t_block = None;
        }
    }
}