//! Explicit diagnostic configuration. No outcome, forecast or RNG input.
use super::shared::game::GameSchedule;
use super::strategy::batting_strategy::{BattingStrategy,BattingPreferences,BattedBallIntent,BattingStrategySettings};
use anyhow::{Result,ensure};
use rusqlite::{Connection,OpenFlags};
use serde::{Serialize,Deserialize};
use std::{collections::{BTreeSet,BTreeMap},path::Path};
use sha2::{Digest,Sha256};

fn assignment_key(seed:u64,kind:&[u8],id:i64)->[u8;32] {
    let mut h=Sha256::new();h.update(b"saberbb:diagnostic-config:balanced-v1\0");
    h.update(seed.to_le_bytes());h.update(kind);h.update(id.to_le_bytes());h.finalize().into()
}

#[derive(Clone,Debug,Serialize,Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TeamObjective { pub team_id:u16, pub strategy:BattingStrategy }
#[derive(Clone,Debug,Serialize,Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlayerPreference { pub player_id:i64, pub preferred_intent:BattedBallIntent }
#[derive(Clone,Debug,Serialize,Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DiagnosticBattingConfig {
    pub version:u8,
    pub default_strategy:BattingStrategy,
    pub default_preferred_intent:BattedBallIntent,
    pub team_strategies:Vec<TeamObjective>,
    pub player_preferences:Vec<PlayerPreference>,
}
impl DiagnosticBattingConfig {
    pub fn parse(bytes:&[u8])->Result<Self> {
        let config:Self=serde_json::from_slice(bytes)?;config.validate()?;Ok(config)
    }
    pub fn validate(&self)->Result<()> {
        ensure!(self.version==1,"unsupported batting configuration version");
        let mut teams=BTreeSet::new();let mut players=BTreeSet::new();
        for t in &self.team_strategies { ensure!(t.team_id>0&&teams.insert(t.team_id),"invalid or duplicate team ID in batting configuration"); }
        for p in &self.player_preferences { ensure!(p.player_id>0&&players.insert(p.player_id),"invalid or duplicate player ID in batting configuration"); }
        Ok(())
    }
    pub fn validate_database(&self,path:&Path)->Result<()> {
        self.validate()?;
        let conn=Connection::open_with_flags(path,OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        for t in &self.team_strategies {
            let exists:bool=conn.query_row("SELECT EXISTS(SELECT 1 FROM team WHERE id=?1)",[t.team_id],|r|r.get(0))?;
            ensure!(exists,"unknown configured team ID {}",t.team_id);
        }
        for p in &self.player_preferences {
            let exists:bool=conn.query_row("SELECT EXISTS(SELECT 1 FROM player_info WHERE id=?1)",[p.player_id],|r|r.get(0))?;
            ensure!(exists,"unknown configured player ID {}",p.player_id);
        }
        Ok(())
    }
    pub fn template(path:&Path)->Result<Self> {
        Self::template_with_seed(path,None)
    }
    pub fn template_with_seed(path:&Path,seed:Option<u64>)->Result<Self> {
        let conn=Connection::open_with_flags(path,OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        conn.execute_batch("BEGIN")?;
        let teams=conn.prepare("SELECT id FROM team ORDER BY id")?.query_map([],|r|Ok(TeamObjective {team_id:r.get(0)?,strategy:BattingStrategy::LetBatterDecide}))?.collect::<rusqlite::Result<Vec<_>>>()?;
        let players=conn.prepare("SELECT id FROM player_info ORDER BY id")?.query_map([],|r|Ok(PlayerPreference {player_id:r.get(0)?,preferred_intent:BattedBallIntent::ContactFirst}))?.collect::<rusqlite::Result<Vec<_>>>()?;
        let mut result=Self {version:1,default_strategy:BattingStrategy::LetBatterDecide,default_preferred_intent:BattedBallIntent::ContactFirst,team_strategies:teams,player_preferences:players};
        if let Some(seed)=seed {
            // Shuffle IDs using independent hashes; keep output rows sorted for editing.
            // Balance purposes globally and the two preferences within each team.
            let mut ids=result.team_strategies.iter().map(|t|t.team_id).collect::<Vec<_>>();
            ids.sort_by_key(|id|(assignment_key(seed,b"team",i64::from(*id)),*id));
            let offset=usize::from(assignment_key(seed,b"team-offset",0)[0])%3;
            let choices=[BattingStrategy::LetBatterDecide,BattingStrategy::PutBallInPlay,BattingStrategy::SeekLineDrive];
            let objectives=ids.iter().enumerate().map(|(rank,id)|(*id,choices[(rank+offset)%3])).collect::<BTreeMap<_,_>>();
            for t in &mut result.team_strategies { t.strategy=objectives[&t.team_id]; }
            let mut rosters:BTreeMap<u16,Vec<i64>>=BTreeMap::new();
            for row in conn.prepare("SELECT team_id,id FROM player_info ORDER BY team_id,id")?.query_map([],|r|Ok((r.get::<_,u16>(0)?,r.get::<_,i64>(1)?)))? {
                let (team,id)=row?;ensure!(objectives.contains_key(&team),"player belongs to an unknown team");rosters.entry(team).or_default().push(id);
            }
            let mut preferences=BTreeMap::new();
            for (team,mut ids) in rosters {
                ids.sort_by_key(|id|(assignment_key(seed,b"player",*id),*id));
                let offset=usize::from(assignment_key(seed,b"preference-offset",i64::from(team))[0])%2;
                for (rank,id) in ids.into_iter().enumerate() {
                    preferences.insert(id,if (rank+offset)%2==0 {BattedBallIntent::ContactFirst}else{BattedBallIntent::LineDrive});
                }
            }
            for p in &mut result.player_preferences { p.preferred_intent=preferences[&p.player_id]; }
        }
        result.validate()?;Ok(result)
    }
    pub fn settings(&self,schedule:&GameSchedule)->Result<BattingStrategySettings> {
        self.validate()?;
        let objective=|id|self.team_strategies.iter().find(|t|t.team_id==id).map(|t|t.strategy).unwrap_or(self.default_strategy);
        let mut settings=BattingStrategySettings {away_strategy:objective(schedule.away_team.id),home_strategy:objective(schedule.home_team.id),player_preferences:Default::default()};
        for team in [&schedule.away_team,&schedule.home_team] {
            for p in &team.players {
                ensure!(p.info.id>0,"configured diagnostic requires positive roster player IDs");
                let intent=self.player_preferences.iter().find(|v|v.player_id==p.info.id).map(|v|v.preferred_intent).unwrap_or(self.default_preferred_intent);
                ensure!(settings.player_preferences.insert(p.info.id,BattingPreferences {preferred_intent:intent}).is_none(),"duplicate roster player ID");
            }
        }
        Ok(settings)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn bytes()->Vec<u8> { br#"{"version":1,"default_strategy":"let_batter_decide","default_preferred_intent":"line_drive","team_strategies":[{"team_id":1,"strategy":"put_ball_in_play"}],"player_preferences":[{"player_id":101,"preferred_intent":"contact_first"}]}"#.to_vec() }
    #[test]
    fn explicit_defaults_and_overrides_keep_coach_and_player_independent() {
        let config=DiagnosticBattingConfig::parse(&bytes()).unwrap();
        let mut schedule=crate::domain::test_support::game_schedule();let settings=config.settings(&schedule).unwrap();
        let overridden=settings.decision(true,1);assert_eq!(overridden.preferences.preferred_intent,BattedBallIntent::LineDrive);assert_eq!(overridden.selected_intent,BattedBallIntent::ContactFirst);
        assert_eq!(settings.decision(false,101).selected_intent,BattedBallIntent::ContactFirst);
        assert_eq!(settings.decision(false,102).selected_intent,BattedBallIntent::LineDrive);
        schedule.away_team.players.reverse();
        assert_eq!(serde_json::to_value(&settings).unwrap(),serde_json::to_value(config.settings(&schedule).unwrap()).unwrap());
    }
    #[test]
    fn invalid_unknown_duplicate_and_outcome_fields_are_rejected() {
        for raw in [String::from_utf8(bytes()).unwrap().replace("\"version\":1","\"version\":2"),
            String::from_utf8(bytes()).unwrap().replace("\"version\":1","\"version\":1,\"version\":1"),
            String::from_utf8(bytes()).unwrap().replace("\"version\":1","\"version\":1,\"fair\":true"),
            String::from_utf8(bytes()).unwrap().replace("\"team_id\":1","\"team_id\":0"),
            String::from_utf8(bytes()).unwrap().replace("\"player_id\":101","\"player_id\":-1")] {
            assert!(DiagnosticBattingConfig::parse(raw.as_bytes()).is_err());
        }
        let mut config=DiagnosticBattingConfig::parse(&bytes()).unwrap();config.team_strategies.push(config.team_strategies[0].clone());assert!(config.validate().is_err());
    }
    #[test]
    fn template_and_id_validation_read_database_without_modifying_it() {
        let id=std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let path=std::env::temp_dir().join(format!("batting-config-{}-{id}.db",std::process::id()));
        let conn=Connection::open(&path).unwrap();
        conn.execute_batch("CREATE TABLE team(id INTEGER PRIMARY KEY);CREATE TABLE player_info(id INTEGER PRIMARY KEY);INSERT INTO team VALUES(1),(2);INSERT INTO player_info VALUES(1),(101);").unwrap();drop(conn);
        let before=std::fs::read(&path).unwrap();let template=DiagnosticBattingConfig::template(&path).unwrap();
        assert_eq!(template.team_strategies.len(),2);assert_eq!(template.player_preferences.len(),2);
        DiagnosticBattingConfig::parse(&bytes()).unwrap().validate_database(&path).unwrap();
        let mut wrong=template.clone();wrong.team_strategies[0].team_id=99;assert!(wrong.validate_database(&path).is_err());
        let mut wrong=template;wrong.player_preferences[0].player_id=999;assert!(wrong.validate_database(&path).is_err());
        assert_eq!(std::fs::read(&path).unwrap(),before);std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn seeded_template_is_reproducible_balanced_and_read_only() {
        let id=std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let path=std::env::temp_dir().join(format!("random-config-{}-{id}.db",std::process::id()));
        let conn=Connection::open(&path).unwrap();
        conn.execute_batch("CREATE TABLE team(id INTEGER PRIMARY KEY);CREATE TABLE player_info(id INTEGER PRIMARY KEY,team_id INTEGER NOT NULL);").unwrap();
        for team in (1..=6).rev() {
            conn.execute("INSERT INTO team VALUES(?1)",[team]).unwrap();
            for player in 1..=5+team%2 { conn.execute("INSERT INTO player_info VALUES(?1,?2)",[team*100+player,team]).unwrap(); }
        }
        drop(conn);let before=std::fs::read(&path).unwrap();
        let a=DiagnosticBattingConfig::template_with_seed(&path,Some(2026110601)).unwrap();
        let b=DiagnosticBattingConfig::template_with_seed(&path,Some(2026110601)).unwrap();
        assert_eq!(serde_json::to_vec(&a).unwrap(),serde_json::to_vec(&b).unwrap());
        assert_ne!(serde_json::to_vec(&a).unwrap(),serde_json::to_vec(&DiagnosticBattingConfig::template_with_seed(&path,Some(2026110602)).unwrap()).unwrap());
        for objective in [BattingStrategy::LetBatterDecide,BattingStrategy::PutBallInPlay,BattingStrategy::SeekLineDrive] {
            assert_eq!(a.team_strategies.iter().filter(|t|t.strategy==objective).count(),2);
        }
        for team in 1..=6 {
            let players=a.player_preferences.iter().filter(|p|p.player_id/100==team).collect::<Vec<_>>();
            let contact=players.iter().filter(|p|p.preferred_intent==BattedBallIntent::ContactFirst).count();
            assert!(contact.abs_diff(players.len()-contact)<=1);
        }
        a.validate_database(&path).unwrap();
        let defaults=DiagnosticBattingConfig::template(&path).unwrap();
        assert!(defaults.team_strategies.iter().all(|t|t.strategy==BattingStrategy::LetBatterDecide));
        assert!(defaults.player_preferences.iter().all(|p|p.preferred_intent==BattedBallIntent::ContactFirst));
        assert_eq!(std::fs::read(&path).unwrap(),before);std::fs::remove_file(path).unwrap();
    }
}
