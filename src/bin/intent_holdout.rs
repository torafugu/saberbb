use anyhow::Result;
use clap::{Parser,Subcommand};
use saberbb::domain::seeded_diagnostic::{self,RunManifest,RunRecorder};
use saberbb::repositories::game_repository::SqlGameRepository;
use std::path::PathBuf;
use saberbb::domain::holdout_schedule;
use saberbb::repositories::db::SqlDb;
use saberbb::domain::diagnostic_batting_config::DiagnosticBattingConfig;
use std::io::Write;

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
        /// Diagnostic assignments from team/player IDs; frozen in the manifest before generation.
        #[arg(long,default_value="default",value_parser=["default","mixed-id-v1"])] batting_profile:String,
        /// Explicit team objectives/player preferences, copied and hashed before generation.
        #[arg(long,conflicts_with="batting_profile")] batting_config:Option<PathBuf>,
    },
    /// Write a new editable config with the current DB team/player IDs (read-only DB).
    ConfigTemplate {
        #[arg(long)] output_file:PathBuf,
        /// Reproducible balanced random assignments; independent from simulation RNG.
        #[arg(long)] assignment_seed:Option<u64>,
    },
    /// Inspect pending rounds and the active season cursor; never changes the database.
    Status { #[arg(long,default_value_t=40)] rounds:u32 },
    /// Re-execute one saved input with the same build fingerprint; never opens a database.
    Replay { #[arg(long)] run_dir:PathBuf, #[arg(long)] game_id:u32 },
}
fn main()->Result<()> {
    let args=Args::parse();
    match args.command {
        Command::Run {seed,rounds,exclude_through_game_id,output_dir,max_pitches,prepare_schedule,batting_profile,batting_config}=>{
            anyhow::ensure!(!output_dir.exists(),"output directory already exists; preserve the previous run and use a new directory");
            anyhow::ensure!(max_pitches>0&&max_pitches<65535,"invalid pitch budget");
            let cfg=saberbb::config::load_app_config()?;
            // Parse and check all explicit IDs before scheduling or opening a run directory.
            let configuration=batting_config.map(|path|->Result<Vec<u8>> {
                let bytes=std::fs::read(path)?;DiagnosticBattingConfig::parse(&bytes)?.validate_database(&cfg.database_path)?;Ok(bytes)
            }).transpose()?;
            saberbb::I18nManager::init(&cfg.language);
            let (status,plan)=holdout_schedule::prepare(&cfg.database_path,rounds,exclude_through_game_id,prepare_schedule,||{
                let mut service=saberbb::domain::schedule_service::ScheduleService {repo:saberbb::repositories::schedule_repository::SqlScheduleRepository::new()?};
                service.schedule_season()
            })?;
            let mut recorder=RunRecorder::start(output_dir,seed,rounds,exclude_through_game_id,max_pitches,cfg.database_path,cfg.language)?;
            if let Some(bytes)=configuration { recorder.freeze_batting_config(&bytes)?; }
            else { recorder.set_batting_profile(&batting_profile)?; }
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
        Command::ConfigTemplate {output_file,assignment_seed}=>{
            let cfg=saberbb::config::load_app_config()?;
            let config=DiagnosticBattingConfig::template_with_seed(&cfg.database_path,assignment_seed)?;
            let mut file=std::fs::OpenOptions::new().create_new(true).write(true).open(&output_file)?;
            file.write_all(&serde_json::to_vec_pretty(&config)?)?;file.sync_all()?;
            println!("{}",serde_json::json!({"status":"template_created","output_file":output_file,"teams":config.team_strategies.len(),"players":config.player_preferences.len(),
                "assignment_seed":assignment_seed.map(|v|v.to_string()),"assignment_method":if assignment_seed.is_some(){"balanced-v1"}else{"uniform-default"}}));
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn config_works_with_implicit_default_but_conflicts_with_explicit_profile() {
        let mut args=vec!["intent_holdout","run","--seed","1","--rounds","1","--exclude-through-game-id","0","--output-dir","new-run","--batting-config","config.json"];
        assert!(Args::try_parse_from(&args).is_ok());
        args.extend(["--batting-profile","mixed-id-v1"]);assert!(Args::try_parse_from(&args).is_err());
        assert!(Args::try_parse_from(["intent_holdout","config-template","--output-file","config.json"]).is_ok());
        assert!(Args::try_parse_from(["intent_holdout","config-template","--output-file","config.json","--assignment-seed","2026110601"]).is_ok());
    }
}
