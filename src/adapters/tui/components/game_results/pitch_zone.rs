use crate::domain::shared::ball::BallLocation;
use ratatui::layout::Rect;
use ratatui::prelude::*;
use ratatui::style::Color;
use ratatui::symbols::Marker;
use ratatui::widgets::canvas::{Canvas, Rectangle};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PitchZoneSection {
    Ball(u8),
    Strike(u8),
}

pub(super) fn draw_strike_zone(frame: &mut Frame, area: Rect, actual_location: BallLocation) {
    let canvas = Canvas::default()
        .marker(Marker::Braille)
        .x_bounds([0.0, area.width as f64])
        .y_bounds([0.0, area.height as f64])
        .paint(|ctx| {
            let active_zone = ball_location_section(actual_location);
            ctx.draw(&Rectangle {
                x: 3.0,
                y: 3.0,
                width: 15.0,
                height: 7.0,
                color: Color::Gray,
            });

            for (zone, x, y, label) in [
                (PitchZoneSection::Ball(1), 1.0, 10.0, "[1]"),
                (PitchZoneSection::Ball(2), 10.0, 10.0, "[2]"),
                (PitchZoneSection::Ball(3), 19.0, 10.0, "[3]"),
                (PitchZoneSection::Ball(4), 1.0, 6.0, "[4]"),
                (PitchZoneSection::Ball(5), 19.0, 6.0, "[5]"),
                (PitchZoneSection::Ball(6), 1.0, 2.0, "[6]"),
                (PitchZoneSection::Ball(7), 10.0, 2.0, "[7]"),
                (PitchZoneSection::Ball(8), 19.0, 2.0, "[8]"),
                (PitchZoneSection::Strike(1), 6.0, 8.0, "<1>"),
                (PitchZoneSection::Strike(2), 10.0, 8.0, "<2>"),
                (PitchZoneSection::Strike(3), 14.0, 8.0, "<3>"),
                (PitchZoneSection::Strike(4), 6.0, 6.0, "<4>"),
                (PitchZoneSection::Strike(5), 10.0, 6.0, "<5>"),
                (PitchZoneSection::Strike(6), 14.0, 6.0, "<6>"),
                (PitchZoneSection::Strike(7), 6.0, 4.0, "<7>"),
                (PitchZoneSection::Strike(8), 10.0, 4.0, "<8>"),
                (PitchZoneSection::Strike(9), 14.0, 4.0, "<9>"),
            ] {
                let color = if zone == active_zone {
                    Color::Yellow
                } else {
                    Color::DarkGray
                };
                ctx.print(
                    x,
                    y,
                    Span::styled(label, color).add_modifier(ratatui::style::Modifier::BOLD),
                );
            }
        });

    frame.render_widget(canvas, area);
}

pub(super) fn ball_location_section(location: BallLocation) -> PitchZoneSection {
    if location.x.abs() <= 1.0 && location.y.abs() <= 1.0 {
        let col = zone_index(location.x, -1.0 / 3.0, 1.0 / 3.0);
        let row = zone_index(-location.y, -1.0 / 3.0, 1.0 / 3.0);

        PitchZoneSection::Strike((row * 3 + col + 1) as u8)
    } else {
        let col = if location.x < -1.0 {
            0
        } else if location.x > 1.0 {
            2
        } else {
            1
        };
        let row = if location.y > 1.0 {
            0
        } else if location.y < -1.0 {
            2
        } else {
            1
        };

        let section = match (row, col) {
            (0, 0) => 1,
            (0, 1) => 2,
            (0, 2) => 3,
            (1, 0) => 4,
            (1, 2) => 5,
            (2, 0) => 6,
            (2, 1) => 7,
            (2, 2) => 8,
            _ => unreachable!("locations inside the strike zone are handled first"),
        };

        PitchZoneSection::Ball(section)
    }
}

fn zone_index(value: f64, low: f64, high: f64) -> usize {
    if value < low {
        0
    } else if value > high {
        2
    } else {
        1
    }
}
