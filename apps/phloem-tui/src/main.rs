mod agent_runtime;
mod app;
mod state;

use crossterm::event::{self, Event};
use ratatui::{DefaultTerminal, Frame};

use crate::app::App;

fn main() -> color_eyre::Result<()> {
    let mut app = App::new();

    app.start()?;
    Ok(())
}

fn app(terminal: &mut DefaultTerminal) -> std::io::Result<()> {
    loop {
        terminal.draw(render)?;
        if crossterm::event::read()?.is_key_press() {
            break Ok(());
        }
    }
}

fn render(frame: &mut Frame) {
    frame.render_widget("hello world", frame.area());
}
