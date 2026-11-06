use anyhow::Result;
use clap::{Parser,Subcommand};
use saberbb::domain::shared::batting_personnel::{BatterCharacter,Manager,ManagerBattingPolicy};
use saberbb::repositories::{db::SqlDb,batting_personnel_repository as repository};
#[derive(Parser)]
#[command(about="Persist batting personalities and independent managers in the configured app DB")]
struct Args {#[command(subcommand)] command:Command}
#[derive(Subcommand)]
enum Command {
    /// Add personnel tables without deleting or resetting data.
    Migrate,
    SetCharacter {#[arg(long)] player_id:i64,#[arg(long,value_parser=["balanced","contact_oriented","line_drive_oriented","adaptive"])] character:String},
    /// Create/update a manager; assigning to a team replaces its current manager, retaining both people.
    SetManager {#[arg(long)] id:i64,#[arg(long)] name:String,#[arg(long,value_parser=["let_batter_decide","put_ball_in_play","seek_line_drive","situational"])] policy:String,#[arg(long)] team_id:Option<u16>},
}
fn main()->Result<()> {
    let args=Args::parse();let db=SqlDb::new()?;let mut conn=db.get_conn()?;
    match args.command {
        Command::Migrate=>repository::migrate(&conn)?,
        Command::SetCharacter {player_id,character}=>{
            let value:BatterCharacter=serde_json::from_value(serde_json::Value::String(character))?;
            repository::save_character(&conn,player_id,value)?;
        }
        Command::SetManager {id,name,policy,team_id}=>{
            let batting_policy:ManagerBattingPolicy=serde_json::from_value(serde_json::Value::String(policy))?;
            repository::save_manager(&mut conn,&Manager {id,name,batting_policy},team_id)?;
        }
    }
    println!("{}",serde_json::json!({"status":"saved"}));Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn accepts_explicit_personnel_commands_and_rejects_unknown_policy() {
        assert!(Args::try_parse_from(["batting_personnel","migrate"]).is_ok());
        assert!(Args::try_parse_from(["batting_personnel","set-character","--player-id","2","--character","adaptive"]).is_ok());
        assert!(Args::try_parse_from(["batting_personnel","set-manager","--id","9","--name","Manager","--policy","situational","--team-id","1"]).is_ok());
        assert!(Args::try_parse_from(["batting_personnel","set-manager","--id","9","--name","Manager","--policy","unknown"]).is_err());
    }
}
