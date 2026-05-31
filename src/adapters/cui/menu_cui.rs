use crossterm::{
    cursor, execute,
    terminal::{self, ClearType},
};
use std::io::{self, Write};

#[macro_export]
macro_rules! rprintln {
    ($($arg:tt)*) => {
        let output = format!($($arg)*);
        for line in output.lines() {
            let _ = crossterm::execute!(
                std::io::stdout(),
                crossterm::cursor::MoveToColumn(0),
                crossterm::terminal::Clear(crossterm::terminal::ClearType::UntilNewLine)
            );
            print!("{}\r\n", line);
        }
        let _ = std::io::Write::flush(&mut std::io::stdout());
    };
}

#[derive(Debug, Clone)]
pub struct MenuItem<T> {
    pub label: String,
    pub value: T,
}

impl<T> std::fmt::Display for MenuItem<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.label)
    }
}

pub fn init_terminal() -> io::Result<()> {
    terminal::enable_raw_mode()?;
    let mut stdout = io::stdout();

    print!("\x1bc");
    let _ = stdout.flush();

    execute!(
        stdout,
        terminal::Clear(ClearType::All),
        cursor::MoveTo(0, 0)
    )?;
    Ok(())
}

pub fn restore_terminal() -> io::Result<()> {
    let mut stdout = io::stdout();

    terminal::disable_raw_mode()?;

    execute!(stdout, cursor::Show,)?;

    println!("\r");
    Ok(())
}
