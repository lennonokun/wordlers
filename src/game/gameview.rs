use std::time::{Duration, Instant};
use std::path::Path;
use std::cmp::min;

use cursive::Cursive;
use cursive::view::Nameable;
use cursive::views::*;
use cursive::theme::{Color, BaseColor, ColorStyle, Effect};
use cursive::traits::*;
use cursive::event::{Event, EventResult, Key};
use cursive::direction::Direction;
use cursive::{Printer, Vec2};
use cursive::view::CannotFocus;

use crate::ds::*;
use super::config::CONFIG;
use super::menu::menu_open;

// TODO how should scrolling and resizing work?

#[derive(PartialEq, Debug)]
enum State {
  Lost, Won, Forfeit, Play
}

struct FbCol {
  ans: Word,
  done: bool,
}

impl FbCol {
  /// draw and return if correct
  fn draw_guess(&self, gw: Word, pos: Vec2, printer: &Printer) -> bool {
    let fb = Feedback::from(gw, self.ans).unwrap();
    for j in 0..gw.wlen {
      let bg = if fb.get_g(j) {
        CONFIG.feedback_correct_bg
      } else if fb.get_y(j) {
        CONFIG.feedback_present_bg
      } else {
        CONFIG.feedback_absent_bg
      };
      let cs = ColorStyle::new(CONFIG.feedback_fg, bg);
      printer.with_color(cs, |printer| {
        printer.print(pos+(j,0), &gw.get(j.into()).unwrap().to_string().as_str())
      });
    }
    fb.is_correct()
  }
}

pub struct GameView {
  wbn: String,
  wbp: String,
  nwords: usize,
  wlen: u8,
  gwb: WBank,
  awb: WBank,
  fbcols: Vec<FbCol>,
  guesses: Vec<Word>,
  answers: Vec<Word>,
  state: State,
  inst: Instant,
  time: Duration,
  nrows: usize,
  ncols: usize,
  guessbuf: String,
  ndone: usize,
  turn: usize,
  last_size: Vec2,
  scroll: usize,
}

impl GameView {
  /// create new feedback col with answer aw
  pub fn new(wbn: &String, wlen: u8, nwords: usize) -> Self {
    let wbp = CONFIG.word_banks.get(wbn).unwrap();
    let (gwb, awb) = WBank::from2(wbp, wlen).unwrap();
    let mut out = Self {
      wbn: wbn.clone(),
      wbp: wbp.clone(),
      nwords,
      wlen,
      gwb,
      awb,
      fbcols: Vec::<FbCol>::new(),
      guesses: Vec::<Word>::new(),
      answers: Vec::<Word>::new(),
      state: State::Play,
      guessbuf: String::new(),
      inst: Instant::now(),
      time: Duration::ZERO,
      nrows: 0,
      ncols: 0,
      ndone: 0,
      turn: 0,
      last_size: Vec2::zero(),
      scroll: 0,
    };
    out.start();
    out
  }

  pub fn start(&mut self) {
    // TODO
    self.guesses.clear();
    self.answers = self.awb.pick(&mut rand::thread_rng(), self.nwords);
    self.fbcols = self.answers.iter()
      .map(|ans| FbCol {ans: *ans, done: false}).collect();
    self.guessbuf.clear();
    self.state = State::Play;
    self.inst = Instant::now();
    self.time = Duration::ZERO;
    self.turn = 0;
  }

  /// guess word
  pub fn guess(&mut self, gw: Word) {
    self.guessbuf = String::new();
    if !self.gwb.data.contains(&gw) {return}

    // inst timing on first guess
    if self.guesses.is_empty() {
      self.inst = Instant::now();
    }

    // add to guesses and increment turn
    self.guesses.push(gw);
    self.turn += 1;

    // update done's and ndone
    for mut fbcol in &mut self.fbcols {
      if !fbcol.done && fbcol.ans == gw {
        fbcol.done = true;
        self.ndone += 1;
      }
    }

    // update state
    if self.ndone == self.nwords {
      self.state = State::Won;
      self.time = self.inst.elapsed();
    } else if self.turn == self.nwords + NEXTRA {
      self.state = State::Lost;
      self.time = self.inst.elapsed();
    }
  }
  
  fn draw_status(&self, printer: &Printer) {
    let limit = self.nwords + NEXTRA;
    let delta = (limit - self.turn) as isize - (self.nwords - self.ndone) as isize;
    let time = if self.guesses.is_empty() {
      0.
    } else {
      self.inst.elapsed().as_millis() as f64 / 1000.
    };

    let s = format!(
      "solved: {}/{}, turns: {}/{} ({:+}), scroll: {}/{}, time: {:.3}s",
      self.ndone,
      self.nwords,
      self.turn,
      limit,
      delta,
      self.scroll,
      self.nrows,
      time,
    );
    printer.print((1,1), &s);
  }

  fn draw_main(&self, printer: &Printer) {
    let maxrow = self.last_size.y - 5;
    for ncol in 0..self.ncols {
      let x = (self.wlen+1) as usize * ncol + 1;
      if let Some(fbc) = self.fbcols.get(self.scroll*self.ncols+ncol) {
        for (y, gw) in self.guesses.iter().enumerate().take(maxrow) {
          let pos: Vec2 = (x, y+3).into();
          if fbc.draw_guess(*gw, pos, printer) {
            break;
          }
        }
        if !fbc.done {
          let y = min(self.guesses.len(), maxrow)+3;
          printer.print((x,y), &self.guessbuf);
        }
      }
    }
  }

  fn draw_results(&self, printer: &Printer) {
    let s_result = match self.state {
      State::Won => "Won",
      State::Lost => "Lost",
      State::Forfeit => "Forfeited",
      State::Play => "", // shouldn't occur
    };

    printer.print((1,1), "Results:");
    printer.print((1,2), &format!(
        "{} on \"{}\" with wlen={}, nwords={}",
        s_result,
        self.wbn,
        self.wlen,
        self.nwords));
    
    printer.print((1,4), "Statistics:");
    printer.print((1,5), &format!(
        "turns: {}/{}, time: {:.3}s",
        self.turn,
        self.nwords + NEXTRA,
        self.time.as_millis() as f64 / 1000.));

    printer.print((1,7), "Answers:");
    for (i, aw) in self.answers.iter().enumerate() {
      let x = (self.wlen as usize+ 1) * (i % self.ncols) + 1;
      let y = i / self.ncols + 8;
      printer.print((x,y), &aw.to_string());
    }

    printer.print((1, self.last_size.y-2),
        "'r': restart, 's': settings, 'q'/Esc: quit");
  }
}

impl View for GameView {
  // TODO add needs redraw

  fn layout(&mut self, size: Vec2) {
    self.last_size = size;
    self.ncols = (size.x-3) / (self.wlen as usize + 1);
    self.nrows = (self.nwords + self.ncols - 1) / self.ncols;
  }

  fn draw(&self, printer: &Printer) {
    printer.print_box((0,0), self.last_size, false);
    if self.state == State::Play {
      self.draw_status(printer);
      self.draw_main(printer);
    } else {
      self.draw_results(printer);
    }
  }

  fn required_size(&mut self, constraint: Vec2) -> Vec2 {
    (constraint.x, constraint.y).into() // TODO limit to be exact with no right space
  }

  fn on_event(&mut self, event: Event) -> EventResult {
    if self.state == State::Play {
      match event {
        Event::Char(c) => if is_alpha(c) {
          self.guessbuf.push(upper(c));
          if self.guessbuf.len() as u8 == self.wlen {
            // TODO why have to clone, the value is lost
            let gw = Word::from(self.guessbuf.clone()).unwrap();
            self.guess(gw);
          }
        } else {
          return EventResult::Ignored;
        } Event::Key(Key::Backspace) => {
          self.guessbuf.pop();
        } Event::Ctrl(Key::Backspace) => {
          self.guessbuf.clear();
        } Event::Key(Key::Up) => {
          self.scroll = (self.scroll + self.nrows - 1) % self.nrows;
        } Event::Key(Key::Down) => {
          self.scroll = (self.scroll + self.nrows + 1) % self.nrows;
        } Event::Key(Key::Esc) => {
          self.state = State::Forfeit;
          self.time = self.inst.elapsed();
        } _ => {
          return EventResult::Ignored;
        }
      }
      EventResult::Consumed(None)
    } else {
      match event {
        Event::Key(Key::Esc) => {
          return EventResult::with_cb(|s| s.quit())
        } Event::Char(c) => if c == 'q' {
          return EventResult::with_cb(|s| s.quit())
        } else if c == 'r' {
          self.start();
        } else if c == 's' {
          return EventResult::with_cb(|s| {s.pop_layer(); menu_open(s)})
        } else {
          return EventResult::Ignored;
        } _ => {
          return EventResult::Ignored;
        }
      }
      EventResult::Consumed(None)
    }
  }

  fn take_focus(&mut self, source: Direction) -> Result<EventResult, CannotFocus> {
    Ok(EventResult::Consumed(None))
  }
}