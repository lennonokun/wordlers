use std::hash::{Hash, Hasher};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::cmp::{min, Ordering};

use rand::Rng;
use rayon::prelude::*;
use pdqselect::select_by;
use fast_math::*;

use super::Cache;
use crate::util::*;

/// solve data
#[derive(Debug, Clone)]
pub struct SData {
  /// cache
  pub cache: Arc<Mutex<Cache>>,
  /// number of top words to try using soft heuristic
  pub ntops1: u32,
  /// number of top words to try using hard heuristic
  pub ntops2: u32,
  /// the relative heuristic difference cut-off
  pub delta: f32,
  /// number of remaining words makes it "endgame"
  pub ecut: u32,
}

impl SData {
  pub fn new(
    cache: Cache,
    ntops1: u32,
    ntops2: u32,
    delta: f32,
    ecut: u32
  ) -> Self {
    let cache = Arc::new(Mutex::new(cache));
    Self {
      cache,
      ntops1,
      ntops2,
      delta,
      ecut,
    }
  }

  pub fn new2(ntops1: u32, ntops2: u32, delta: f32) -> Self {
    let cache = Cache::new(64, 16);
    Self::new(cache, ntops1, ntops2, delta, 15)
  }

  pub fn deep_clone(&self) -> Self {
    let cache2 = (*self.cache.lock().unwrap()).clone();
    Self::new(cache2, self.ntops1, self.ntops2, self.delta, self.ecut)
  }
}

#[derive(Debug, Clone, Copy)]
pub struct ScoredWord {
  pub w: Word,
  pub h: f32,
}

// reversed, we want to select the highest h, not lowest
fn cmp_scored(sw1: &ScoredWord, sw2: &ScoredWord) -> Ordering {
  (&sw2.h).partial_cmp(&sw1.h).unwrap()
}

#[derive(Clone)]
struct SolveGivenData {
  pub fbmap: FbMap<DTree>,
  pub tot: u32,
  pub impossible: bool,
}

#[derive(Clone)]
struct SolveAllData {
  pub dt: Option<DTree>,
  pub beta: u32,
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct State {
  // arc because easy mode keeps guesses constant
  pub gws: Arc<Vec<Word>>,
  pub aws: Vec<Word>,
  pub wlen: u8,
  pub turns: u32,
  pub hard: bool,
}

pub fn fb_filter(gw: Word, fb: Feedback, gws: &Vec<Word>) -> Vec<Word> {
  gws
    .iter()
    .cloned()
    .filter(|gw2| fb == Feedback::from(gw, *gw2).unwrap())
    .collect()
}

impl State {
  pub fn new(wbank: &WBank, turns: Option<u32>, hard: bool) -> Self {
    Self {
      gws: Arc::new(wbank.gws.clone()),
      aws: wbank.aws.clone(),
      wlen: wbank.wlen,
      turns: turns.unwrap_or(wbank.wlen as u32 + NEXTRA as u32),
      hard,
    }
  }

  pub fn child(&self, gws: Arc<Vec<Word>>, aws: Vec<Word>) -> Self {
    Self {
      gws,
      aws,
      wlen: self.wlen,
      turns: self.turns - 1,
      hard: self.hard,
    }
  }

  pub fn fb_follow(self, gw: Word, fb: Feedback) -> Self {
    let gws = if self.hard {
      Arc::new(fb_filter(gw, fb, &self.gws))
    } else {
      self.gws.clone()
    };
    let aws = fb_filter(gw, fb, &self.aws);
    self.child(gws, aws)
  }

  pub fn fb_partition(&self, gw: &Word) -> FbMap<(Option<Vec<Word>>, Vec<Word>)> {
    let default_entry = (self.hard.then(Vec::new), Vec::new());
    let mut fbmap = FbMap::new();
    // partition gws if hard
    if self.hard {
      for gw2 in &*self.gws {
        let mut entry = fbmap.entry(Feedback::from(*gw, *gw2).unwrap());
        entry.or_insert_with(|| default_entry.clone()).0
          .as_mut().unwrap().push(*gw2);
      }
    }
    // partition aws
    for aw in &self.aws {
      let mut entry = fbmap.entry(Feedback::from(*gw, *aw).unwrap());
      entry.or_insert_with(|| default_entry.clone()).1.push(*aw);
    }
    fbmap
  }

//  pub fn fb_counts(&self, gw: &Word) -> Vec<u16> {
//    let mut counts = vec![0; 3usize.pow(self.wlen as u32)];
//    for aw in self.aws.iter().cloned() {
//      counts[fb_id(*gw, aw) as usize] += 1;
//    }
//    counts
//  }

  pub fn fb_unique(&self, gw: Word) -> bool {
    let mut set = HashSet::new();
    for aw in self.aws.iter().cloned() {
      if !set.insert(fb_id(gw, aw)) { return false; }
    }
    true
  }

  pub fn letter_evals(&self) -> (Vec<Vec<f32>>, Vec<f32>) {
    // get letter counts
    let mut gss = vec![vec![0f32; self.wlen as usize]; 26];
    let mut ys = vec![0f32; 26];
    for aw in &self.aws {
      for i in 0..(self.wlen as usize) {
        gss[aw.data[i] as usize][i] += 1.;
        if !aw.data[0..i].contains(&aw.data[i]) {
          ys[aw.data[i] as usize] += 1.;
        }
      }
    }

    // compute entropy
    let n = self.aws.len() as f32;
    let entropy = |m: f32| {
      if m == 0. { return 0.}
      - (m/n) * log2_raw(m/n)
    };
    for i in 0..26 {
      for j in 0..(self.wlen as usize) {
        gss[i][j] = entropy(gss[i][j]);
      }
      ys[i] = entropy(ys[i]);
    }

    (gss, ys)
  }

  pub fn heuristic1(&self, gw: &Word, gss: &Vec<Vec<f32>>, ys: &Vec<f32>) -> f32 {
    let mut h = 0f32;
    for i in 0..(self.wlen as usize) {
      h += gss[gw.data[i] as usize][i];
      if !gw.data[0..i].contains(&gw.data[i]) {
        h += ys[gw.data[i] as usize];
      }
    }

    // naive
    if self.aws.contains(&gw) {
      h * 1.05
    } else {
      h
    }
  }

  // tot = sum(ax+b)
  //     = a*sum(x)+n*b
  //     = alen+n*b
  // alen is constant and b is negative, so just weigh by n
  // correct h by 1 if gw is in aws
  // |b| is much larger than 1, so just use b=-2
  // fuzzier than using precomputed averages, but faster
  // could be parallelized
  pub fn heuristic2(&self, gw: &Word, sd: &SData) -> f32 {
    let mut parts = vec![false; 3usize.pow(self.wlen as u32)];
    let mut nparts = 0;
    for aw in self.aws.iter().cloned() {
      let i = fb_id(*gw, aw) as usize;
      if !parts[i] {
        nparts += 1;
        parts[i] = true;
      }
    }

    // slow-ish
    if self.aws.contains(gw) {
      (2*nparts + 1) as f32
    } else {
      (2*nparts) as f32
    }
  }

  pub fn top_words(&self, sd: &SData) -> Vec<ScoredWord> {
    let glen = self.gws.len();
    let alen = self.aws.len();
    let ntops1 = min(sd.ntops1 as usize, glen);
    let ntops2 = if self.hard {2 * sd.ntops2 as usize} else {sd.ntops2 as usize};
    let ntops2 = min(ntops2, glen);
    let (gss, ys) = self.letter_evals();

    // select ntops1 with fast heuristic
    let mut tops: Vec<ScoredWord> = (&self.gws)
      .par_iter()
      .map(|gw| {
        ScoredWord {w: *gw, h: self.heuristic1(&gw, &gss, &ys)}
      })
      .collect();
    
//    let ntops1 = if alen < 10 {
//      500
//    } else if alen < 25 {
//      1000
//    } else {
//      3000
//    };

    // select ntops1 with fast heuristic
    select_by(&mut tops, ntops1-1, &mut cmp_scored);
    
    // select ntops2 with slow heuristic, and stop if best found
    let best = Mutex::new(None);
    (&mut tops[0..ntops1]).par_iter_mut().for_each(|sw| {
      if best.lock().unwrap().is_none() {
        (*sw).h = self.heuristic2(&sw.w, sd);
        // stop if best is found
        if (*sw).h >= (2*alen) as f32 {
          *best.lock().unwrap() = Some(*sw);
        }
      }
    });

    // just return a vec with the best word if found
    if let Some(sw) = *best.lock().unwrap() {
      return vec![sw];
    }

    select_by(&mut tops[0..ntops1], ntops2-1, &mut cmp_scored);
    tops.truncate(ntops2);

    tops.sort_by(cmp_scored);
    if tops[0].h >= (2*alen - 2) as f32 {
      vec![tops[0]]
    } else {
      tops
    }

  }

  pub fn solve_given(&self, gw: Word, sd: &SData, beta: u32) -> Option<DTree> {
    let alen = self.aws.len();

    // leaf if guessed
    if alen == 1 && gw == *self.aws.get(0).unwrap() {
      return Some(DTree::Leaf);
    }
    // impossible guesses
    if self.turns == 0 || (self.turns == 1 && alen > 1) {
      return None;
    }
    // check alpha = 2|A|-1
    if beta <= 2 * (alen as u32) - 1 {
      return None;
    }

    let sgdata = Mutex::new(SolveGivenData {
      fbmap: FbMap::new(),
      tot: alen as u32,
      impossible: false,
    });

    let mut fbp = self.fb_partition(&gw);
    fbp.into_par_iter().for_each(|(fb, (ogws, aws))| {
      if aws.is_empty() {
        return;
      } else if sgdata.lock().unwrap().impossible {
        return;
      } else if fb.is_correct() {
        let fbmap = &mut sgdata.lock().unwrap().fbmap;
        fbmap.insert(fb, DTree::Leaf);
        return;
      }

      // make state
      let gws = ogws.map(|gws| Arc::new(gws)).unwrap_or_else(|| self.gws.clone());
      let state2 = self.child(gws, aws);

      let tot = sgdata.lock().unwrap().tot.clone();
      match state2.solve(sd, beta - tot) {
        None => {
          sgdata.lock().unwrap().impossible = true;
        }, Some(dt) => {
          let mut sgdata = sgdata.lock().unwrap();
          sgdata.tot += dt.get_tot();
          sgdata.fbmap.insert(fb, dt);
          sgdata.impossible |= sgdata.tot >= beta;
        }
      }
    });

    let sgdata = sgdata.into_inner().unwrap();
    if sgdata.impossible {
      None
    } else {
      Some(DTree::Node {
        tot: sgdata.tot,
        word: gw,
        fbmap: sgdata.fbmap,
      })
    }
  }

  pub fn solve(&self, sd: &SData, beta: u32) -> Option<DTree> {
    let alen = self.aws.len();

    // no more turns
    if self.turns == 0 {
      return None;
    }
    // check alpha = 2|A|-1
    if beta <= 2 * alen as u32 - 1 {
      return None;
    }
    // trivial decision tree for one answer
    if let [aw] = &self.aws[..] {
      return Some(DTree::Node {
        tot: 1,
        word: *aw,
        fbmap: [(Feedback::from_str("GGGGG").unwrap(), DTree::Leaf)].into(),
      });
    }
    // check endgame if viable
    if alen <= sd.ecut as usize {
      for aw in self.aws.iter().cloned() {
        if self.fb_unique(aw) {
          return self.solve_given(aw, sd, beta);
        }
      }
    }
    // check cache
    if !self.hard {
      let mut cache = sd.cache.lock().unwrap();
      if let Some(dt) = cache.read(self) {
        return Some(dt.clone());
      }
    }

    // finally, check top words
    let sad = Mutex::new(SolveAllData {dt: None, beta});
    let tops = self.top_words(&sd);
    let top_h = tops[0].h;
    tops
      .into_iter()
      .take_while(|sw| (top_h - sw.h) / top_h < sd.delta)
      .par_bridge()
      .for_each(|sw| {
      // stop early if best possible is found
      let sad2 = sad.lock().unwrap().clone();
      if sad2.beta <= 2 * alen as u32 {return}
      // solve and update best + beta
      let dt2 = self.solve_given(sw.w, sd, sad2.beta);
      if let Some(dt2) = dt2 {
        let mut sad = sad.lock().unwrap();
        if dt2.get_tot() < sad.beta {
          sad.beta = dt2.get_tot();
          sad.dt = Some(dt2);
        }
      }
    });

    let sad = sad.into_inner().unwrap();
    let dt = sad.dt;

    // add to cache
    if !self.hard {
      if let Some(ref dt) = dt {
        let mut cache = sd.cache.lock().unwrap();
        cache.add(self.clone(), dt.clone());
      }
    }

    dt
  }
}

impl Hash for State {
  fn hash<H: Hasher>(&self, h: &mut H) {
    self.turns.hash(h);
    self.aws.hash(h);
  }
}

#[cfg(test)]
mod test {
  use super::*;

//  #[test]
//  fn check_news() {
//    let (gwb, awb) = WBank::from2("/usr/share/hustle/bank1.csv", 5).unwrap();
//
//    let state1 = State::new(gwb.data.clone(), awb.data.clone(), 5, false);
//    let state2 = State::new2(Arc::new(gwb.data.clone()), awb.data.clone(), 5, 6, false);
//    let state3 = State::new3();
//    assert_eq!(state1, state2);
//    assert_eq!(state2, state3);
//  }
//
  #[test]
  fn simple_solve() {
    let wbank = WBank::load1().unwrap();
    let state1 = State::new(&wbank, None, false);
    let state2 = State::new(&wbank, None, true);
    let sd = SData::new2(1000, 10);

    assert!(state1.solve(&sd, u32::MAX).is_some());
    assert!(state2.solve(&sd, u32::MAX).is_some());
  }

  #[test]
  fn impossible_solve() {
    let wbank = WBank::load1().unwrap();
    let state = State::new(&wbank, Some(2), false);
    let sd = SData::new2(1000, 10);

    // should not be able to solve bank1 in two turns
    assert!(state.solve(&sd, u32::MAX).is_none());
  }
}
