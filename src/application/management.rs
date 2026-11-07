//! Management use cases shared by the CLI and the terminal management console.
use anyhow::{Context, Result};
use crate::app_context;
use crate::domain::{game_service::GameService, manager_service::ManagerService,
    player_factory::PlayerFactory, player_service::PlayerService,
    random_provider::RealRng, schedule_service::ScheduleService};
use crate::repositories::manager_repository::SqlManagerRepository;
use crate::I18nManager;

#[derive(Clone, Debug)]
pub struct GenerationReport { pub players: u16, pub managers: usize }

pub fn generate_characters(players: u16) -> Result<GenerationReport> {
    if players > 0 {
        let service = PlayerService { repo: app_context().player_repository.clone() };
        let mut factory = PlayerFactory::new(service);
        factory.load_player_probs()?;
        for index in 0..players {
            factory.generate_and_save_player()
                .with_context(|| format!("player generation failed after {index} saved players"))?;
        }
    }
    let mut managers = ManagerService { repo: SqlManagerRepository::new()? };
    let managers = managers.generate_missing_managers(&I18nManager::global().lang_db(), &mut RealRng::new())?;
    Ok(GenerationReport { players, managers })
}

pub fn schedule_seasons(seasons: u8) -> Result<()> {
    let mut service = ScheduleService { repo: app_context().schedule_repository.clone() };
    for index in 0..seasons {
        service.schedule_season().with_context(|| format!("scheduling failed after {index} saved seasons"))?;
    }
    Ok(())
}

pub fn process_rounds(rounds: u16, analysis: bool) -> Result<()> {
    let mut service = GameService { repo: app_context().game_repository.clone() };
    for index in 0..rounds {
        let result = if analysis { service.analyze_game_round() } else { service.process_game_round() };
        result.with_context(|| format!("processing failed after {index} completed rounds"))?;
    }
    Ok(())
}
