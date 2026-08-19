use saberbb::domain::player_factory::PlayerFactory;
use saberbb::domain::player_service::PlayerService;
use saberbb::domain::resolver::fielding_physics::FielderRiskTolerance;
use saberbb::domain::shared::game_state::*;
use saberbb::domain::shared::player::*;
use saberbb::domain::shared::stadium::*;
use saberbb::domain::util::*;
use saberbb::repositories::player_repository::SqlPlayerRepository;

pub fn generate_stadium() -> Stadium {
    Stadium::new(1, "AAA".to_string(), 98.0, 120.0, 2.0)
}

pub fn generate_default_fielders() -> [ActiveFielder; 9] {
    let p = ActiveFielder {
        position: Position::P,
        id: 0,
        info: PlayerFactory::<SqlPlayerRepository>::default_fielder_info(FielderType::Pitcher),
        polar_position: PolarPosition::new(MOUND_DISTANCE, 0.0),
        risk_tolerance: FielderRiskTolerance::Balanced,
    };

    let c = ActiveFielder {
        position: Position::C,
        id: 1,
        info: PlayerFactory::<SqlPlayerRepository>::default_fielder_info(FielderType::Catcher),
        polar_position: PolarPosition::new(0.0, 0.0),
        risk_tolerance: FielderRiskTolerance::Balanced,
    };

    let fb = ActiveFielder {
        position: Position::FB,
        id: 2,
        info: PlayerFactory::<SqlPlayerRepository>::default_fielder_info(
            FielderType::CornerInfielder,
        ),
        polar_position: PolarPosition::new(35.0, 33.0),
        risk_tolerance: FielderRiskTolerance::Balanced,
    };

    let sb = ActiveFielder {
        position: Position::SB,
        id: 3,
        info: PlayerFactory::<SqlPlayerRepository>::default_fielder_info(
            FielderType::MiddleInfielder,
        ),
        polar_position: PolarPosition::new(40.0, 18.0),
        risk_tolerance: FielderRiskTolerance::Balanced,
    };

    let tb = ActiveFielder {
        position: Position::TB,
        id: 4,
        info: PlayerFactory::<SqlPlayerRepository>::default_fielder_info(
            FielderType::CornerInfielder,
        ),
        polar_position: PolarPosition::new(35.0, -33.0),
        risk_tolerance: FielderRiskTolerance::Balanced,
    };

    let ss = ActiveFielder {
        position: Position::SS,
        id: 5,
        info: PlayerFactory::<SqlPlayerRepository>::default_fielder_info(
            FielderType::MiddleInfielder,
        ),
        polar_position: PolarPosition::new(40.0, -18.0),
        risk_tolerance: FielderRiskTolerance::Balanced,
    };

    let rf = ActiveFielder {
        position: Position::RF,
        id: 6,
        info: PlayerFactory::<SqlPlayerRepository>::default_fielder_info(FielderType::Outfielder),
        polar_position: PolarPosition::new(80.0, 26.0),
        risk_tolerance: FielderRiskTolerance::Balanced,
    };

    let cf = ActiveFielder {
        position: Position::CF,
        id: 7,
        info: PlayerFactory::<SqlPlayerRepository>::default_fielder_info(FielderType::Outfielder),
        polar_position: PolarPosition::new(90.0, 0.0),
        risk_tolerance: FielderRiskTolerance::Balanced,
    };

    let lf = ActiveFielder {
        position: Position::LF,
        id: 8,
        info: PlayerFactory::<SqlPlayerRepository>::default_fielder_info(FielderType::Outfielder),
        polar_position: PolarPosition::new(80.0, -26.0),
        risk_tolerance: FielderRiskTolerance::Balanced,
    };

    [p, c, fb, sb, tb, ss, rf, cf, lf]
}

pub fn generate_batter() -> BatterInfo {
    let player_service = PlayerService {
        repo: SqlPlayerRepository::new().expect("failed to initialize player repository"),
    };
    let mut player_factory = PlayerFactory::new(player_service);
    player_factory
        .load_player_probs()
        .expect("failed to load player probabilities");
    player_factory
        .assign_batter_info()
        .expect("failed to generate batter info")
}

pub fn generate_pitcher() -> PitcherInfo {
    let player_service = PlayerService {
        repo: SqlPlayerRepository::new().expect("failed to initialize player repository"),
    };
    let mut player_factory = PlayerFactory::new(player_service);
    player_factory
        .load_player_probs()
        .expect("failed to load player probabilities");

    loop {
        if let Some(pitcher_info) = player_factory
            .generate_player()
            .expect("failed to generate player")
            .defense_skills
            .pitcher
        {
            return pitcher_info;
        }
    }
}

pub fn generate_catcher() -> CatcherInfo {
    CatcherInfo {
        fielder_info: PlayerFactory::<SqlPlayerRepository>::default_fielder_info(
            FielderType::Catcher,
        ),
    }
}

pub fn generate_runner() -> ActiveRunner {
    let player_service = PlayerService {
        repo: SqlPlayerRepository::new().expect("failed to initialize player repository"),
    };
    let mut player_factory = PlayerFactory::new(player_service);
    player_factory
        .load_player_probs()
        .expect("failed to load player probabilities");
    let player = player_factory
        .generate_player()
        .expect("failed to generate player");

    ActiveRunner {
        id: player.info.id,
        skills: player.offense_skills.running,
    }
}
