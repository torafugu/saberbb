use anyhow::Result;
use clap::{Parser,Subcommand};
use saberbb::domain::seeded_diagnostic::{self,RunManifest,RunRecorder};
use saberbb::repositories::game_repository::SqlGameRepository;
use std::path::PathBuf;
use saberbb::domain::holdout_schedule;
use saberbb::repositories::db::SqlDb;

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
        /// Schedule missing future seasons and align the cursor to the predeclared pending rounds.
        #[arg(long)] prepare_schedule:bool,
    },
    /// Inspect pending rounds and the active season cursor; never changes the database.
    Status { #[arg(long,default_value_t=40)] rounds:u32 },
    /// Re-execute one saved input with the same build fingerprint; never opens a database.
    Replay { #[arg(long)] run_dir:PathBuf, #[arg(long)] game_id:u32 },
}
fn main()->Result<()> {
    let args=Args::parse();
    match args.command {
        Command::Run {seed,rounds,exclude_through_game_id,output_dir,max_pitches,prepare_schedule}=>{
            anyhow::ensure!(!output_dir.exists(),"output directory already exists; preserve the previous run and use a new directory");
            anyhow::ensure!(max_pitches>0&&max_pitches<65535,"invalid pitch budget");
            let cfg=saberbb::config::load_app_config()?;
            saberbb::I18nManager::init(&cfg.language);
            let (status,plan)=holdout_schedule::prepare(&cfg.database_path,rounds,exclude_through_game_id,prepare_schedule,||{
                let mut service=saberbb::domain::schedule_service::ScheduleService {repo:saberbb::repositories::schedule_repository::SqlScheduleRepository::new()?};
                service.schedule_season()
            })?;
            let mut recorder=RunRecorder::start(output_dir,seed,rounds,exclude_through_game_id,max_pitches,cfg.database_path,cfg.language)?;
            recorder.set_round_plan(plan.clone())?;
            let mut repo=SqlGameRepository::new()?;
            let cursor_db=SqlDb::new()?;let mut expected=(status.current_season,status.current_round_seq);
            seeded_diagnostic::process_run_with_prepare(&mut repo,&mut recorder,|index|{
                let next=&plan[index];
                if prepare_schedule {
                    let mut conn=cursor_db.get_conn()?;holdout_schedule::align_cursor(&mut conn,expected,next)?;
                    expected=(next.season,next.round_seq.checked_add(1).ok_or_else(||anyhow::anyhow!("round cursor overflow"))?);
                }
                Ok(())
            })?;
            println!("{}",serde_json::json!({"status":"complete","manifest":recorder.output.join("manifest.json"),"games":recorder.manifest.games.len()}));
        }
        Command::Status {rounds}=>{
            let cfg=saberbb::config::load_app_config()?;let status=holdout_schedule::read_status(&cfg.database_path)?;
            println!("{}",serde_json::json!({"database_path":cfg.database_path,"requested_rounds":rounds,
                "available_pending_rounds":status.pending_rounds.len(),"enough_pending_rounds":status.pending_rounds.len()>=rounds as usize,"schedule":status}));
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
