use crate::t;

pub fn display_game_seasons_scheduled(num_of_season: i8) {
    println!(
        "{}",
        t!("view_game_result_this_season", "num_of_season" => num_of_season.to_string())
    );
}
