use anyhow::Result;
use clap::{Parser,Subcommand};
use saberbb::domain::seeded_diagnostic::{self,RunManifest,RunRecorder};
use saberbb::repositories::game_repository::SqlGameRepository;
use std::path::PathBuf;

#[derive(Parser)]
#[command(about="Generate explicit-seed intent holdout records, or replay a frozen case without DB writes")]
struct Args { #[command(subcommand)] command:Command }
#[derive(Subcommand)]
enum Command {
    /// Process the next scheduled rounds, saving ordinary game results and a provenance journal.
    Run {
        #[arg(long)] seed:u64,
        #[arg(long)] rounds:u32,
        #[arg(long)] exclude_through_game_id:u32,
        #[arg(long)] output_dir:PathBuf,
        #[arg(long,default_value_t=10000)] max_pitches:u32,
    },
    /// Re-execute one saved input with the same build fingerprint; never opens a database.
    Replay { #[arg(long)] run_dir:PathBuf, #[arg(long)] game_id:u32 },
}
fn main()->Result<()> {
    let args=Args::parse();
    match args.command {
        Command::Run {seed,rounds,exclude_through_game_id,output_dir,max_pitches}=>{
            let cfg=saberbb::config::load_app_config()?;
            saberbb::I18nManager::init(&cfg.language);
            let mut recorder=RunRecorder::start(output_dir,seed,rounds,exclude_through_game_id,max_pitches,cfg.database_path,cfg.language)?;
            let mut repo=SqlGameRepository::new()?;
            seeded_diagnostic::process_run(&mut repo,&mut recorder)?;
            println!("{}",serde_json::json!({"status":"complete","manifest":recorder.output.join("manifest.json"),"games":recorder.manifest.games.len()}));
        }
        Command::Replay {run_dir,game_id}=>{
            let manifest:RunManifest=serde_json::from_slice(&std::fs::read(run_dir.join("manifest.json"))?)?;
            saberbb::I18nManager::init(&manifest.language);
            let result_sha=seeded_diagnostic::replay(&run_dir,game_id)?;
            println!("{}",serde_json::json!({"status":"replay_matched","game_id":game_id,"result_sha256":result_sha}));
        }
    }
    Ok(())
}
