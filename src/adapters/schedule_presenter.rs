use crate::t;

pub fn display_game_seasons_scheduled(num_of_season: i8) {
    println!(
        "{}",
        t!("game_seasons_scheduled", "num_of_season" => num_of_season.to_string())
    );
}
