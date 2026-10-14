mod action;
mod app;
mod columns;
mod diffs;
mod dump;
mod element_tip;
mod field;
mod grid;
mod ids;
mod keys;
mod login;
mod markdown;
mod model;
mod nav;
mod palette;
mod panels;
mod peek;
mod pictures;
mod review;
mod runtime;
mod settings;
mod status;
mod text;
mod theme;
mod viewer;
mod views;
mod widgets;
mod work;

use domain::Root;
use platform::{StartError, Title};

use crate::app::App;

pub fn run(root: &Root) -> Result<(), StartError> {
    let title = Title::new(format!("review - {}", root.as_path().display()));
    platform::run(title, App::new(root))
}

#[cfg(test)]
mod tests;
