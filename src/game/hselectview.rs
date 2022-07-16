use std::fmt::Display;
use std::rc::Rc;

use cursive::Cursive;
use cursive::view::Nameable;
use cursive::views::*;
use cursive::theme::{Color, BaseColor, ColorStyle, Effect};
use cursive::traits::*;
use cursive::event::{Event, EventResult, Key};
use cursive::direction::Direction;
use cursive::{Printer, Vec2};
use cursive::view::CannotFocus;

// TODO how should scrolling and resizing work?

#[derive(Debug)]
struct Item<T> {
  pub label: String,
  pub value: Rc<T>,
}

impl<T> Item<T> {
  fn new(label: String, value: T) -> Self {
    let value = Rc::new(value);
    Item { label, value }
  }
}

#[derive(Debug)]
pub struct HSelectView<T> {
  items: Vec<Item<T>>,
  index: usize, 
}

impl<T> HSelectView<T> {
  pub fn new() -> Self {
    Self {
      items: Vec::new(),
      index: 0,
    }
  }

  pub fn add_item(&mut self, label: String, value: T) {
    self.items.push(Item::new(label, value));
  }

  // todo is cloning bad? have to clone in draw
  pub fn selected_label(&self) -> Option<String> {
    self.items.get(self.index).map(|item| item.label.clone())
  }

  pub fn selection(&self) -> Option<Rc<T>> {
    eprintln!("selection!");
    self.items.get(self.index).map(|item| item.value.clone())
  }
}

impl<T: 'static> View for HSelectView<T> {
  // TODO add needs redraw

  fn draw(&self, printer: &Printer) {
    if let Some(label) = self.selected_label() {
      printer.print((0,0), &label);
    }
  }

  fn on_event(&mut self, event: Event) -> EventResult {
    match event {
      Event::Key(Key::Left) => {
        self.index = (self.index + 1) % self.items.len();
      } Event::Key(Key::Right) => {
        self.index = (self.index + self.items.len() - 1) % self.items.len();
      } _ => {
        return EventResult::Ignored;
      }
    }
    EventResult::Consumed(None)
  }

  fn take_focus(&mut self, source: Direction) -> Result<EventResult, CannotFocus> {
    Ok(EventResult::Consumed(None))
  }
}
