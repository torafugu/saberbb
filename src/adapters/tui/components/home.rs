use super::Component;
use crate::adapters::tui::action::{Action, MenuOption};
use crate::application::management;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{prelude::*, widgets::*};
use tokio::sync::mpsc::UnboundedSender;

pub struct Home {
    tx: Option<UnboundedSender<Action>>,
    selected: usize,
    amount: String,
    editing: bool,
    busy: bool,
    status: String,
}
impl Home {
    const MENU: [MenuOption; 4] = [MenuOption::Schedule, MenuOption::Generate, MenuOption::Process, MenuOption::Analyze];
    pub fn new() -> Self {
        Self { tx: None, selected: 0, amount: "1".into(), editing:false, busy: false,
            status: "Results and statistics: cargo run --features desktop -- --gui".into() }
    }
    fn submit(&mut self) {
        let Ok(amount) = self.amount.parse::<u16>() else {
            self.status = "Enter a number from 0 to 65535.".into(); return;
        };
        let task = Self::MENU[self.selected];
        if task == MenuOption::Schedule && (amount == 0 || amount > u8::MAX as u16) {
            self.status = "Seasons must be between 1 and 255.".into(); return;
        }
        if matches!(task, MenuOption::Process | MenuOption::Analyze) && amount == 0 {
            self.status = "Rounds must be at least 1.".into(); return;
        }
        let Some(tx) = self.tx.clone() else { return; };
        self.busy = true;
        self.status = format!("Running {task} ({amount})…");
        // Database/simulation work never runs on the terminal event loop.
        tokio::spawn(async move {
            let result = tokio::task::spawn_blocking(move || -> anyhow::Result<String> {
                match task {
                    MenuOption::Schedule => { management::schedule_seasons(amount as u8)?; Ok(format!("Scheduled {amount} seasons.")) }
                    MenuOption::Generate => { let r = management::generate_characters(amount)?; Ok(format!("Generated {} players; assigned {} managers.", r.players, r.managers)) }
                    MenuOption::Process | MenuOption::Analyze => { management::process_rounds(amount, task == MenuOption::Analyze)?; Ok(format!("Processed {amount} rounds.")) }
                }
            }).await;
            let message = match result {
                Ok(Ok(message)) => message,
                Ok(Err(error)) => format!("Failed: {error:#}. Earlier completed work is retained."),
                Err(error) => format!("Worker failed: {error}. Check saved progress before retrying."),
            };
            let _ = tx.send(Action::ManagementFinished(message));
        });
    }
}
impl Component for Home {
    fn is_busy(&self) -> bool { self.busy }
    fn register_action_handler(&mut self, tx: UnboundedSender<Action>) -> color_eyre::Result<()> { self.tx = Some(tx); Ok(()) }
    fn handle_key_event(&mut self, key: KeyEvent) -> color_eyre::Result<Option<Action>> {
        if self.busy { return Ok(None); }
        match key.code {
            KeyCode::Char(c) if c.is_ascii_digit() && (!self.editing || self.amount.len() < 5) => {
                if !self.editing || self.amount == "0" { self.amount.clear(); }
                self.editing = true;
                self.amount.push(c);
            }
            KeyCode::Backspace => { self.amount.pop(); self.editing=true; }
            KeyCode::Char('c') => { self.amount.clear(); self.editing=true; },
            _ => {}
        }
        Ok(None)
    }
    fn update(&mut self, action: Action) -> color_eyre::Result<Option<Action>> {
        if let Action::ManagementFinished(message) = action {
            self.busy = false; self.editing=false; self.status = message; return Ok(None);
        }
        if self.busy { return Ok(None); }
        match action {
            Action::SelectNext => {self.selected = (self.selected + 1) % Self::MENU.len();self.editing=false;},
            Action::SelectPrevious => {self.selected = (self.selected + Self::MENU.len() - 1) % Self::MENU.len();self.editing=false;},
            Action::ConfirmSelection => self.submit(),
            _ => {}
        }
        Ok(None)
    }
    fn draw(&mut self, frame: &mut Frame, area: Rect) -> color_eyre::Result<()> {
        let [title, menu, input, status, help] = Layout::vertical([
            Constraint::Length(3), Constraint::Length(7), Constraint::Length(3), Constraint::Min(4), Constraint::Length(3),
        ]).areas(area);
        frame.render_widget(Paragraph::new("SABERBB  /  Management console").block(Block::bordered()), title);
        let mut state = ListState::default(); state.select(Some(self.selected));
        frame.render_stateful_widget(List::new(Self::MENU.iter().map(|m| ListItem::new(m.to_string())))
            .block(Block::bordered().title("Operations"))
            .highlight_style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
            .highlight_symbol("▶ "), menu, &mut state);
        frame.render_widget(Paragraph::new(format!("Count: {}", self.amount)).block(Block::bordered()), input);
        frame.render_widget(Paragraph::new(self.status.as_str()).wrap(Wrap { trim: false }).block(Block::bordered().title("Status")), status);
        frame.render_widget(Paragraph::new("↑↓ select · c clear · digits count · Enter execute · q quit\nGenerate 0: fill manager vacancies. Process: no contact diagnostics. Analyze: records diagnostics."), help);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn management_menu_wraps_and_busy_jobs_ignore_new_commands() {
        let mut home=Home::new();
        home.update(Action::SelectPrevious).unwrap();assert_eq!(home.selected,3);
        home.update(Action::SelectNext).unwrap();assert_eq!(home.selected,0);
        home.busy=true;home.update(Action::SelectNext).unwrap();assert_eq!(home.selected,0);
        home.update(Action::ManagementFinished("Done".into())).unwrap();assert!(!home.is_busy());
    }
    #[test]
    fn invalid_counts_do_not_start_management_work() {
        let mut home=Home::new();home.amount="0".into();home.submit();
        assert!(home.status.contains("between 1 and 255"));assert!(!home.is_busy());
        home.selected=2;home.submit();assert!(home.status.contains("at least 1"));
    }
}
