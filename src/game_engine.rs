use super::repositories::game_repository::{load_game_round_to_process, save_game_round};
use crate::domains::game::{Count, Inning, MAX_INNING, MAX_OUT};
use crate::domains::player::Batter;
use crate::domains::types::{BattingResult, InningType};
use crate::domains::utils::next_tb;
use crate::resolver::batting_resolve;
use crate::t;
use anyhow::{Context, Result};
use std::sync::Arc;

pub fn process_game() -> Result<()> {
    // 1. Get game round to process
    let mut game_round = load_game_round_to_process()
        .context(t!("error", "function" => "load_game_round_to_process"))?;

    // 2. Procees games in the game round
    for game in game_round.games.iter_mut() {
        let mut _is_in_game: bool = true;
        let mut _inning_seq: i8 = 1;
        let mut _inning_tb: InningType = InningType::TOP;
        let mut _top_total_score: i8 = 0;
        let mut _bottom_total_score: i8 = 0;
        let mut _top_batter_order: usize = 1;
        let mut _bottom_batter_order: usize = 1;

        game.away_batters = Vec::from([
            Batter::new(1, "Top batter 1", 1.0, -0.5),
            Batter::new(2, "Top batter 2", 1.2, -0.8),
            Batter::new(3, "Top batter 3", 1.4, 0.8),
            Batter::new(4, "Top batter 4", 1.6, 1.0),
            Batter::new(5, "Top batter 5", 1.5, 0.9),
            Batter::new(6, "Top batter 6", -0.1, 0.2),
            Batter::new(7, "Top batter 7", 0.1, -0.3),
            Batter::new(8, "Top batter 8", -1.0, -0.5),
            Batter::new(9, "Top batter 9", -1.2, -1.2),
        ]);
        game.home_batters = Vec::from([
            Batter::new(10, "Bottom batter 1", 0.9, -0.8),
            Batter::new(11, "Bottom batter 2", 1.1, -0.6),
            Batter::new(12, "Bottom batter 3", 1.2, 1.0),
            Batter::new(13, "Bottom batter 4", 1.4, 1.4),
            Batter::new(14, "Bottom batter 5", 0.2, 1.1),
            Batter::new(15, "Bottom batter 6", -0.5, -0.2),
            Batter::new(16, "Bottom batter 7", -0.8, -0.1),
            Batter::new(17, "Bottom batter 8", -1.3, -0.3),
            Batter::new(18, "Bottom batter 9", -1.4, -0.4),
        ]);

        // loop for an innning
        while _is_in_game {
            let mut _inning: Inning = Inning {
                tb: _inning_tb,
                seq: _inning_seq,
                counts: Vec::new(),
                point: 0,
            };
            let mut _count_seq = 0;
            let mut _is_first_runner = false;
            let mut _is_second_runner = false;
            let mut _is_third_runner = false;
            let mut _out_count = 0;

            while _out_count < MAX_OUT {
                _count_seq += 1;

                let _current_batter: &Batter;
                if _inning_tb == InningType::TOP {
                    _current_batter = &game.away_batters[_top_batter_order - 1];
                } else {
                    _current_batter = &game.home_batters[_bottom_batter_order - 1];
                }

                let mut _count = Count {
                    seq: _count_seq,
                    is_first_runner: _is_first_runner,
                    is_second_runner: _is_second_runner,
                    is_third_runner: _is_third_runner,
                    batter: Arc::new(_current_batter.clone()),
                    result: BattingResult::OUT,
                    point: 0,
                    out: _out_count,
                };

                // Batting result calculation
                _count.result = batting_resolve(_count.batter.clone());

                match _count.result {
                    BattingResult::SINGLE => {
                        if _is_third_runner {
                            _count.point += 1;
                            _inning.point += 1;
                            _is_third_runner = false;
                        }
                        if _is_second_runner {
                            _is_second_runner = false;
                            _is_third_runner = true;
                        }
                        if _is_first_runner {
                            _is_second_runner = true;
                        }
                        _is_first_runner = true;
                    }
                    BattingResult::DOUBLE => {
                        if _is_third_runner {
                            _count.point += 1;
                            _inning.point += 1;
                            _is_third_runner = false;
                        }
                        if _is_second_runner {
                            _count.point += 1;
                            _inning.point += 1;
                        }
                        if _is_first_runner {
                            _is_first_runner = false;
                            _is_third_runner = true;
                        }
                        _is_second_runner = true;
                    }
                    BattingResult::TRIPLE => {
                        if _is_third_runner {
                            _count.point += 1;
                            _inning.point += 1;
                        }
                        if _is_second_runner {
                            _count.point += 1;
                            _inning.point += 1;
                            _is_second_runner = false;
                        }
                        if _is_first_runner {
                            _count.point += 1;
                            _inning.point += 1;
                            _is_first_runner = false;
                        }
                        _is_third_runner = true;
                    }
                    BattingResult::HOMERUN => {
                        if _is_third_runner {
                            _count.point += 1;
                            _inning.point += 1;
                            _is_third_runner = false;
                        }
                        if _is_second_runner {
                            _count.point += 1;
                            _inning.point += 1;
                            _is_second_runner = false;
                        }
                        if _is_first_runner {
                            _count.point += 1;
                            _inning.point += 1;
                            _is_first_runner = false;
                        }
                        _count.point += 1;
                        _inning.point += 1;
                    }
                    _ => {
                        _count.result = BattingResult::OUT;
                        if _out_count < MAX_OUT {
                            _out_count += 1;
                        }
                    }
                }

                if _inning_tb == InningType::TOP {
                    _top_total_score += _count.point;
                    if _top_batter_order == 9 {
                        _top_batter_order = 1;
                    } else {
                        _top_batter_order += 1;
                    }
                } else {
                    _bottom_total_score += _count.point;
                    if _bottom_batter_order == 9 {
                        _bottom_batter_order = 1;
                    } else {
                        _bottom_batter_order += 1;
                    }
                }
                _count.is_first_runner = _is_first_runner;
                _count.is_second_runner = _is_second_runner;
                _count.is_third_runner = _is_third_runner;
                _count.out = _out_count;
                _inning.counts.push(_count);

                // Check walk-off
                if _inning_seq == MAX_INNING
                    && _inning_tb == InningType::BOTTOM
                    && _bottom_total_score > _top_total_score
                {
                    _is_in_game = false;
                    break;
                }
            }

            game.innings.push(_inning);

            // Check Game-Set
            if _inning_seq == MAX_INNING {
                if _inning_tb == InningType::BOTTOM {
                    break;
                } else if _bottom_total_score > _top_total_score {
                    break;
                }
            } else {
                if _inning_tb == InningType::BOTTOM {
                    _inning_seq += 1;
                }
            }
            _inning_tb = next_tb(_inning_tb);
        }
    }

    if let Err(e) = save_game_round(&game_round) {
        eprintln!("{}:{}", t!("error", "function" => "save_game_round"), e);
    }

    Ok(())
}
