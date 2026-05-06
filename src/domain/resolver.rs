use crate::domain::shared::player::Player;
use crate::domain::shared::types::BattingResult;

pub fn batting_resolve(batter: &Player) -> BattingResult {
    let rng: f64 = rand::random();
    let _result: BattingResult;
    // TODO : Adjust by mod_slg!
    let xbh_average: f64 = batter.slg() - batter.hit_average();
    let double_average: f64 = batter.hit_average() + xbh_average * 0.5;
    let triple_average: f64 = batter.hit_average() + xbh_average * 0.6;
    let home_run_average: f64 = batter.hit_average() + xbh_average;

    match rng {
        n if batter.hit_average() > n => _result = BattingResult::Single,
        n if double_average > n => _result = BattingResult::Double,
        n if triple_average > n => _result = BattingResult::Triple,
        n if home_run_average > n => _result = BattingResult::HomeRun,
        _ => _result = BattingResult::Out,
    }
    _result
}
