/**                          AAPM@Audio-8151 Library                          */
/**
 * Copyright 2024 HaמuL
 * Description: Library for AAPM@Audio-8151(Fourier Analogue-in-Digital) codec
 */

pub mod backend;
pub mod fourier;
pub mod tools;

pub mod common;
pub mod encode;
pub mod decode;
pub mod repair;

pub use encode::Encode;
pub use decode::Decode;
pub use repair::Repair;