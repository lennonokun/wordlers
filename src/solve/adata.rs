use std::fs::File;
use std::io::{BufRead, BufReader, Result};
use std::path::Path;

/// analysis data, including heuristics and lower bounds
#[derive(Debug, Clone)]
pub struct AData {
  approxs: Vec<f32>,
  lbounds: Vec<u32>,
}

impl AData {
  pub fn load<P>(hdp: &P, ldp: &P) -> Result<Self>
  where P: AsRef<Path> + ?Sized, {
    let hd_reader = BufReader::new(File::open(hdp)?);
    let ld_reader = BufReader::new(File::open(ldp)?);
    let approxs = hd_reader
      .lines()
      .skip(1)
      .filter_map(|s| {
        let s = s.ok()?;
        let s = s.split(",").nth(1)?;
        s.parse::<f32>().ok()
      })
      .collect::<Vec<f32>>();
    let lbounds = ld_reader
      .lines()
      .skip(1)
      .filter_map(|s| {
        let s = s.ok()?;
        let s = s.split(",").nth(1)?;
        s.parse::<u32>().ok()
      })
      .collect::<Vec<u32>>();

    Ok(Self {approxs, lbounds})
  }

  #[inline]
  pub fn get_approx(&self, n: usize) -> Option<f32> {
    self.approxs.get(n-1).map(|x| *x)
  }

  #[inline]
  pub fn get_lbound(&self, n: usize) -> Option<u32> {
    self.lbounds.get(n-1).map(|x| *x)
  }
}
