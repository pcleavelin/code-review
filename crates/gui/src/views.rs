mod bars;
mod center;
mod console;
mod inbox;
mod left;
mod markdown;
mod palette;
mod panel_tree;
mod references;
mod review;
mod settings;
mod sheet;
mod stack;
mod table;
mod tooltip;
mod viewer;

use crate::model::Model;
use crate::widgets::{Container, Frame};

pub(crate) use panel_tree::panel_input;

pub(crate) fn build(model: &Model, frame: &mut Frame<'_>) {
    frame.start(Container::Window);
    bars::top_bar(model, frame);
    frame.start(Container::Body);
    panel_tree::tree(model, frame);
    frame.finish();
    bars::status_bar(model, frame);
    frame.finish();
    panel_tree::drag_band(model, frame);
    panel_tree::picker(model, frame);
    palette::palette(model, frame);
    settings::settings(model, frame);
    viewer::viewer(model, frame);
    tooltip::tooltip(model, frame);
}
