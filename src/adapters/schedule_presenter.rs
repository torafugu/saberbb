use crate::t;

pub fn display_game_seasons_scheduled(num_of_seasons: i8) {
    println!(
        "{}",
        t!("game_seasons_scheduled", "num_of_seasons" => num_of_seasons.to_string())
    );
}
