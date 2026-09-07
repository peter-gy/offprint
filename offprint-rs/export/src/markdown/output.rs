use std::cell::Cell;
use std::ops::Deref;
use std::rc::Rc;

use offprint_model::{ErrorStage, Result};

use crate::markdown_byte_limit_error;

pub(super) struct Budget {
    maximum: u64,
    used: Cell<u64>,
    exceeded: Cell<bool>,
}

impl Budget {
    pub(super) fn new(maximum: u64) -> Rc<Self> {
        Rc::new(Self {
            maximum,
            used: Cell::new(0),
            exceeded: Cell::new(false),
        })
    }

    pub(super) fn claim(&self, bytes: usize) -> Result<()> {
        let used = self.used.get().saturating_add(bytes as u64);
        if used > self.maximum || self.exceeded.get() {
            self.exceeded.set(true);
            return Err(markdown_byte_limit_error(ErrorStage::Encoding));
        }
        self.used.set(used);
        Ok(())
    }

    fn release(&self, bytes: usize) {
        self.used.set(self.used.get().saturating_sub(bytes as u64));
    }

    pub(super) fn check(&self) -> Result<()> {
        if self.exceeded.get() {
            Err(markdown_byte_limit_error(ErrorStage::Encoding))
        } else {
            Ok(())
        }
    }
}

pub(super) struct Output {
    text: String,
    budget: Rc<Budget>,
}

impl Output {
    pub(super) fn new(budget: &Rc<Budget>) -> Self {
        Self {
            text: String::new(),
            budget: Rc::clone(budget),
        }
    }

    pub(super) fn push_str(&mut self, text: &str) {
        if self.budget.claim(text.len()).is_ok() {
            self.text.push_str(text);
        }
    }

    pub(super) fn push(&mut self, character: char) {
        self.push_str(character.encode_utf8(&mut [0; 4]));
    }

    pub(super) fn pop(&mut self) {
        if let Some(character) = self.text.pop() {
            self.budget.release(character.len_utf8());
        }
    }

    pub(super) fn truncate(&mut self, length: usize) {
        let previous = self.text.len();
        self.text.truncate(length);
        self.budget.release(previous - self.text.len());
    }

    pub(super) fn trim_whitespace(&mut self) {
        self.truncate(self.text.trim_end().len());
        let leading = self.text.len() - self.text.trim_start().len();
        self.text.drain(..leading);
        self.budget.release(leading);
    }

    pub(super) fn finish(mut self) -> Result<String> {
        self.budget.check()?;
        self.budget.release(self.text.len());
        Ok(std::mem::take(&mut self.text))
    }
}

impl Deref for Output {
    type Target = str;

    fn deref(&self) -> &str {
        &self.text
    }
}

impl Drop for Output {
    fn drop(&mut self) {
        self.budget.release(self.text.len());
    }
}
