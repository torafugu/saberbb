//! Explicit-seed diagnostic runs. Frozen pre-game inputs, replay, and DB-log provenance.
use crate::domain::random_provider::RealRng;
use crate::domain::shared::game::{GameResult,GameSchedule};
use crate::domain::shared::contact_state::ContactStateRecord;
use crate::domain::strategy::batting_strategy::{BattingStrategySettings,BattingStrategy,BattingPreferences,BattedBallIntent};
use crate::repositories::game_repository::GameRoundRepository;
use crate::domain::holdout_schedule::PlannedRound;
use crate::domain::diagnostic_batting_config::DiagnosticBattingConfig;
use anyhow::{Result,Context,bail,ensure};
use serde::{Serialize,Deserialize};
use sha2::{Digest,Sha256};
use std::{fs::{self,OpenOptions},io::Write,path::{Path,PathBuf}};

pub const ENGINE: &str=env!("SABERBB_ENGINE_SOURCE_SHA256");
pub const BUILD: &str=env!("SABERBB_BUILD_SHA256");
pub const SOURCE: &str="seeded_intent_holdout_v1";
fn default_profile()->String { "default".into() }
pub fn settings_for(profile:&str,schedule:&GameSchedule)->Result<BattingStrategySettings> {
    let mut settings=BattingStrategySettings::default();
    if profile=="default" { return Ok(settings); }
    if profile=="player-context-v1" {
        let mut ids=std::collections::BTreeSet::new();
        for team in [&schedule.away_team,&schedule.home_team] {
            if let Some(manager)=&team.manager {manager.validate().map_err(anyhow::Error::msg)?;}
            for player in &team.players {ensure!(player.info.id>0&&ids.insert(player.info.id),"invalid or duplicate player identity in derived profile");}
        }
        if let (Some(a),Some(h))=(&schedule.away_team.manager,&schedule.home_team.manager) {ensure!(a.id!=h.id,"same manager assigned to both opposing teams");}
        return Ok(BattingStrategySettings::player_driven());
    }
    ensure!(profile=="mixed-id-v1","unsupported batting profile");
    // ID-only diagnostic assignments, independent of abilities, outcomes and RNG.
    let strategy=|id:u16|match id%3 {0=>BattingStrategy::LetBatterDecide,1=>BattingStrategy::PutBallInPlay,_=>BattingStrategy::SeekLineDrive};
    settings.away_strategy=strategy(schedule.away_team.id);settings.home_strategy=strategy(schedule.home_team.id);
    for team in [&schedule.away_team,&schedule.home_team] {
        for player in &team.players {
            ensure!(player.info.id>0,"mixed profile requires persisted positive player IDs");
            let preference=BattingPreferences {preferred_intent:if player.info.id%2==0 {BattedBallIntent::ContactFirst}else{BattedBallIntent::LineDrive}};
            ensure!(settings.player_preferences.insert(player.info.id,preference).is_none(),"duplicate roster player ID");
        }
    }
    Ok(settings)
}
pub fn derived_seed(master:u64,game_id:u32)->u64 {
    let mut h=Sha256::new();h.update(b"saberbb:holdout:v1\0");h.update(master.to_le_bytes());h.update(game_id.to_le_bytes());
    u64::from_le_bytes(h.finalize()[..8].try_into().unwrap())
}
pub fn sha(bytes:&[u8])->String { format!("{:x}",Sha256::digest(bytes)) }
fn canonical<T:Serialize>(value:&T)->Result<Vec<u8>> { Ok(serde_json::to_vec(&serde_json::to_value(value)?)?) }
fn fresh_json<T:Serialize>(path:&Path,value:&T)->Result<String> {
    let bytes=canonical(value)?;
    let mut file=OpenOptions::new().create_new(true).write(true).open(path)?;
    file.write_all(&bytes)?;file.sync_all()?;Ok(sha(&bytes))
}
fn optional(h:&mut Sha256,value:Option<&str>) {
    if let Some(s)=value { h.update([1]);h.update((s.len() as u64).to_le_bytes());h.update(s.as_bytes()); }
    else { h.update([0]); }
}
pub fn contacts_sha256(records:&[ContactStateRecord])->Result<String> {
    let mut h=Sha256::new();let mut previous=None;
    for r in records {
        ensure!(previous.is_none_or(|p|r.count_seq>p),"contact records must be strictly ordered");previous=Some(r.count_seq);
        h.update(r.count_seq.to_le_bytes());h.update([r.state_version]);
        h.update((r.state_source.len() as u64).to_le_bytes());h.update(r.state_source.as_bytes());
        optional(&mut h,r.invalid_reason.as_deref());
        let payload=r.state.as_ref().map(serde_json::to_string).transpose()?;
        optional(&mut h,payload.as_deref());
    }
    Ok(format!("{:x}",h.finalize()))
}

#[derive(Clone,Debug,Serialize,Deserialize)]
pub struct GameCase {
    pub version:u8,
    pub engine_source_sha256:String,
    pub build_sha256:String,
    pub game_seed:String,
    pub schedule:GameSchedule,
    pub batting_settings:BattingStrategySettings,
    pub max_pitches:u32,
}
#[derive(Clone,Debug,Serialize,Deserialize)]
pub struct CaseRecord {
    pub game_id:u32,
    pub game_seed:String,
    pub input_file:String,
    pub input_sha256:String,
    pub status:String,
    pub result_sha256:Option<String>,
    pub contacts_sha256:Option<String>,
    pub contact_states:Option<usize>,
}
#[derive(Clone,Debug,Serialize,Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BattingConfigSource { pub file:String, pub sha256:String }

#[derive(Clone,Debug,Serialize,Deserialize)]
pub struct RunManifest {
    #[serde(default,skip_serializing_if="Option::is_none")]
    pub batting_config:Option<BattingConfigSource>,
    #[serde(default="default_profile")]
    pub batting_profile:String,
    pub version:u8,
    pub source:String,
    pub status:String,
    pub master_seed:String,
    pub seed_derivation:String,
    pub engine_source_sha256:String,
    pub build_sha256:String,
    pub build_target:String,
    pub build_profile:String,
    pub compiler:String,
    pub git_commit:String,
    pub git_dirty:bool,
    pub cargo_lock_sha256:String,
    pub model:String,
    pub frozen_policy_sha256:String,
    pub source_db_path:PathBuf,
    pub language:String,
    pub requested_rounds:u32,
    pub completed_rounds:u32,
    pub exclude_through_game_id:u32,
    pub max_pitches:u32,
    pub selection:String,
    pub games:Vec<CaseRecord>,
    #[serde(default,skip_serializing_if="Option::is_none")]
    pub round_plan:Option<Vec<PlannedRound>>,
    pub error:Option<String>,
}
pub struct RunRecorder { pub manifest:RunManifest, pub output:PathBuf }
fn manifest_settings(manifest:&RunManifest,output:&Path,schedule:&GameSchedule)->Result<BattingStrategySettings> {
    if manifest.batting_profile=="configured-v1" {
        let source=manifest.batting_config.as_ref().context("missing frozen batting configuration")?;
        ensure!(source.file=="batting_config.json","unexpected batting configuration path");
        let bytes=fs::read(output.join(&source.file))?;
        ensure!(sha(&bytes)==source.sha256,"frozen batting configuration hash mismatch");
        DiagnosticBattingConfig::parse(&bytes)?.settings(schedule)
    } else {
        ensure!(manifest.batting_config.is_none(),"configuration supplied for an incompatible profile");
        settings_for(&manifest.batting_profile,schedule)
    }
}
impl RunRecorder {
    pub fn start(output:PathBuf,seed:u64,rounds:u32,exclude:u32,max_pitches:u32,db_path:PathBuf,language:String)->Result<Self> {
        ensure!(rounds>0&&max_pitches>0&&max_pitches<65535,"invalid run rounds or pitch budget");
        fs::create_dir(&output).context("output directory must be new (no overwrite)")?;
        fs::create_dir(output.join("cases"))?;
        let recorder=Self { output,manifest:RunManifest { batting_config:None,batting_profile:default_profile(),version:1,source:SOURCE.into(),status:"running".into(),
            master_seed:seed.to_string(),seed_derivation:"SHA256(saberbb:holdout:v1\\0 + u64le(master) + u32le(game)); first 8 bytes u64le".into(),
            engine_source_sha256:ENGINE.into(),build_sha256:BUILD.into(),build_target:env!("SABERBB_BUILD_TARGET").into(),
            build_profile:env!("SABERBB_BUILD_PROFILE").into(),compiler:env!("SABERBB_BUILD_COMPILER").into(),
            git_commit:env!("SABERBB_GIT_COMMIT").into(),git_dirty:env!("SABERBB_GIT_DIRTY")=="true",cargo_lock_sha256:sha(include_bytes!("../../Cargo.lock")),
            model:crate::domain::shared::finite_bat_collision::MODEL.into(),
            frozen_policy_sha256:sha(include_bytes!("../../tools/diagnostics/pre_swing_aim_seed2_frozen.json")),source_db_path:db_path,language,
            requested_rounds:rounds,completed_rounds:0,exclude_through_game_id:exclude,max_pitches,
            selection:"next requested scheduled rounds; every unprocessed game, no outcome-based filtering".into(),games:Vec::new(),round_plan:None,error:None } };
        recorder.save()?;Ok(recorder)
    }
    fn save(&self)->Result<()> {
        let tmp=self.output.join("manifest.next.json");
        fresh_json(&tmp,&self.manifest)?;
        // Windows does not replace an existing destination on rename.
        // Keep the last complete journal beside it until the replacement succeeds.
        let path=self.output.join("manifest.json");let backup=self.output.join("manifest.previous.json");
        if path.exists() { fs::rename(&path,&backup)?; }
        match fs::rename(&tmp,&path) {
            Ok(())=>{ if backup.exists() { fs::remove_file(backup)?; } Ok(()) },
            Err(error)=>{ if backup.exists() { let _=fs::rename(backup,&path); } Err(error.into()) },
        }
    }
    pub fn begin(&mut self,schedule:&GameSchedule)->Result<GameCase> {
        ensure!(schedule.id>self.manifest.exclude_through_game_id,"game {} overlaps the excluded development/parity range",schedule.id);
        ensure!(!self.manifest.games.iter().any(|g|g.game_id==schedule.id),"duplicate game in run");
        let seed=derived_seed(self.manifest.master_seed.parse()?,schedule.id);
        let case=GameCase { version:1,engine_source_sha256:ENGINE.into(),build_sha256:BUILD.into(),game_seed:seed.to_string(),
            schedule:schedule.clone(),batting_settings:manifest_settings(&self.manifest,&self.output,schedule)?,max_pitches:self.manifest.max_pitches };
        let file=format!("cases/game_{}.json",schedule.id);
        let hash=fresh_json(&self.output.join(&file),&case)?;
        self.manifest.games.push(CaseRecord { game_id:schedule.id,game_seed:seed.to_string(),input_file:file,input_sha256:hash,
            status:"prepared".into(),result_sha256:None,contacts_sha256:None,contact_states:None });
        self.save()?;
        // First execution and replay both consume the exact persisted JSON bytes.
        Ok(serde_json::from_slice(&fs::read(self.output.join(format!("cases/game_{}.json",schedule.id)))?)?)
    }
    pub fn set_round_plan(&mut self,plan:Vec<PlannedRound>)->Result<()> {
        ensure!(self.manifest.games.is_empty()&&plan.len()==self.manifest.requested_rounds as usize,"round plan must be fixed before generation");
        let ids=plan.iter().flat_map(|p|p.game_ids.iter().copied()).collect::<Vec<_>>();
        let unique=ids.iter().copied().collect::<std::collections::BTreeSet<_>>();
        ensure!(ids.len()==unique.len()&&ids.iter().all(|id|*id>self.manifest.exclude_through_game_id)&&plan.iter().all(|p|!p.game_ids.is_empty()),"invalid, duplicate or excluded round plan games");
        self.manifest.round_plan=Some(plan);self.save()
    }
    pub fn set_batting_profile(&mut self,profile:&str)->Result<()> {
        ensure!(self.manifest.games.is_empty()&&self.manifest.batting_config.is_none()&&["default","mixed-id-v1","player-context-v1"].contains(&profile),"invalid or late batting profile");
        self.manifest.batting_profile=profile.into();self.save()
    }
    pub fn freeze_batting_config(&mut self,bytes:&[u8])->Result<()> {
        ensure!(self.manifest.games.is_empty()&&self.manifest.batting_config.is_none(),"configuration must be frozen before generation");
        DiagnosticBattingConfig::parse(bytes)?;
        let file="batting_config.json";
        let mut output=OpenOptions::new().create_new(true).write(true).open(self.output.join(file))?;
        output.write_all(bytes)?;output.sync_all()?;
        self.manifest.batting_profile="configured-v1".into();
        self.manifest.batting_config=Some(BattingConfigSource {file:file.into(),sha256:sha(bytes)});self.save()
    }
    fn saved(&mut self,result:&GameResult)->Result<()> {
        let r=self.manifest.games.iter_mut().find(|g|g.game_id==result.id).context("unprepared saved game")?;
        r.result_sha256=Some(sha(&canonical(result)?));r.contacts_sha256=Some(contacts_sha256(&result.player_contact_states)?);
        r.contact_states=Some(result.player_contact_states.len());r.status="saved".into();self.save()
    }
    pub fn fail(&mut self,error:&str)->Result<()> { self.manifest.status="failed".into();self.manifest.error=Some(error.into());self.save() }
    pub fn complete(&mut self)->Result<()> {
        ensure!(self.manifest.completed_rounds==self.manifest.requested_rounds&&!self.manifest.games.is_empty(),"run is incomplete");
        ensure!(self.manifest.games.iter().all(|g|g.status=="saved"),"unsaved games in run");
        self.manifest.status="complete".into();self.save()
    }
}

/// Shared GameService loop with a declared seed, settings and pitch budget.
pub fn simulate_case(case:&GameCase)->Result<GameResult> {
    ensure!(case.version==1&&case.engine_source_sha256==ENGINE&&case.build_sha256==BUILD,"case engine/build fingerprint mismatch");
    let seed:u64=case.game_seed.parse()?;
    crate::domain::game_service::simulate_game(case.schedule.clone(),Box::new(RealRng::from_seed(seed)),
        case.batting_settings.clone(),Some(case.max_pitches),true)
}

pub fn process_run<R:GameRoundRepository>(repo:&mut R,recorder:&mut RunRecorder)->Result<()> {
    process_run_with_prepare(repo,recorder,|_|Ok(()))
}
pub fn process_run_with_prepare<R:GameRoundRepository>(repo:&mut R,recorder:&mut RunRecorder,mut prepare:impl FnMut(usize)->Result<()>)->Result<()> {
    let result=(||->Result<()> {
        for index in 0..recorder.manifest.requested_rounds {
            prepare(index as usize)?;
            let schedules=repo.load_game_schedules_to_process()?;
            ensure!(!schedules.is_empty(),"no scheduled games; no empty rounds accepted");
            if let Some(plan)=recorder.manifest.round_plan.as_ref() {
                let p=&plan[index as usize];let mut ids=schedules.iter().map(|s|s.id).collect::<Vec<_>>();ids.sort();
                let mut expected=p.game_ids.clone();expected.sort();
                ensure!(ids==expected&&schedules.iter().all(|s|s.season==p.season&&s.round_seq==p.round_seq),"schedule changed after preflight; refusing a different game population");
            }
            // Prepare every input in the round before any result is generated or saved.
            let cases=schedules.iter().map(|s|recorder.begin(s)).collect::<Result<Vec<_>>>()?;
            for case in cases {
                let result=simulate_case(&case)?;
                repo.update_game_result(&result)?;recorder.saved(&result)?;
            }
            repo.update_current_round_seq()?;
            recorder.manifest.completed_rounds+=1;recorder.save()?;
        }
        recorder.complete()
    })();
    if let Err(error)=&result { let _=recorder.fail(&format!("{error:#}")); }
    result
}

pub fn replay(run_dir:&Path,game_id:u32)->Result<String> {
    let manifest:RunManifest=serde_json::from_slice(&fs::read(run_dir.join("manifest.json"))?)?;
    ensure!(manifest.status=="complete"&&manifest.engine_source_sha256==ENGINE&&manifest.build_sha256==BUILD,"run incomplete or engine/build mismatch");
    let record=manifest.games.iter().find(|g|g.game_id==game_id).context("game not present in run")?;
    let expected_file=format!("cases/game_{game_id}.json");ensure!(record.input_file==expected_file,"unexpected case path");
    let bytes=fs::read(run_dir.join(&record.input_file))?;
    ensure!(sha(&bytes)==record.input_sha256,"saved case hash mismatch");
    let case:GameCase=serde_json::from_slice(&bytes)?;
    ensure!(canonical(&case.batting_settings)?==canonical(&manifest_settings(&manifest,run_dir,&case.schedule)?)?,"saved batting settings do not match frozen configuration");
    ensure!(case.schedule.id==game_id&&case.game_seed==record.game_seed,"case identity mismatch");
    ensure!(case.game_seed==derived_seed(manifest.master_seed.parse()?,game_id).to_string(),"derived seed mismatch");
    let result=simulate_case(&case)?;
    let fingerprint=sha(&canonical(&result)?);
    if record.result_sha256.as_deref()!=Some(&fingerprint) { bail!("replayed full result hash mismatch"); }
    ensure!(record.contacts_sha256.as_deref()==Some(&contacts_sha256(&result.player_contact_states)?),"replayed diagnostic hash mismatch");
    Ok(fingerprint)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn player_context_case_replays_and_only_changes_diagnostic_logs() {
        use crate::domain::shared::batting_personnel::{BatterCharacter,Manager,ManagerBattingPolicy};
        let path=folder();let mut run=RunRecorder::start(path.clone(),2026110602,1,0,2000,"test.db".into(),"en-US".into()).unwrap();
        run.set_batting_profile("player-context-v1").unwrap();
        let mut schedule=crate::domain::test_support::game_schedule();schedule.max_inning=1;
        for p in &mut schedule.away_team.players {p.batting_character=BatterCharacter::Adaptive;}
        for p in &mut schedule.home_team.players {p.batting_character=BatterCharacter::LineDriveOriented;}
        schedule.away_team.manager=Some(Manager {id:91,name:"Away manager".into(),batting_policy:ManagerBattingPolicy::Situational});
        schedule.home_team.manager=Some(Manager {id:92,name:"Home manager".into(),batting_policy:ManagerBattingPolicy::PutBallInPlay});
        let case=run.begin(&schedule).unwrap();let result=simulate_case(&case).unwrap();
        let mut seen=0;
        for r in &result.player_contact_states {
            if let Some(state)=&r.state {
                let trace=state.batting_selection.as_ref().unwrap();let top=state.intent_batter.as_ref().unwrap().is_top;
                assert_eq!(trace.manager.as_ref().unwrap().id,if top {91}else{92});
                assert_eq!(trace.character,if top {BatterCharacter::Adaptive}else{BatterCharacter::LineDriveOriented});
                if let Some(observed)=&state.observed_intent_plans {assert_eq!(canonical(&trace.decision).unwrap(),canonical(&observed.decision).unwrap());}
                seen+=1;
            }
        }
        assert!(seen>0);let mut fixed=case.clone();fixed.batting_settings=BattingStrategySettings::default();
        let mut a=serde_json::to_value(&result).unwrap();let mut b=serde_json::to_value(simulate_case(&fixed).unwrap()).unwrap();
        a.as_object_mut().unwrap().remove("player_contact_states");b.as_object_mut().unwrap().remove("player_contact_states");assert_eq!(a,b);
        run.saved(&result).unwrap();run.manifest.completed_rounds=1;run.complete().unwrap();
        assert_eq!(replay(&path,schedule.id).unwrap(),sha(&canonical(&result).unwrap()));fs::remove_dir_all(path).unwrap();
    }
    #[test]
    fn configured_case_freezes_exact_bytes_replays_and_detects_tampering() {
        let path=folder();let mut run=RunRecorder::start(path.clone(),2026110601,1,0,2000,"test.db".into(),"en-US".into()).unwrap();
        let bytes=br#"{ "version":1,"default_strategy":"let_batter_decide","default_preferred_intent":"line_drive","team_strategies":[{"team_id":1,"strategy":"put_ball_in_play"}],"player_preferences":[{"player_id":101,"preferred_intent":"contact_first"}] }"#;
        run.freeze_batting_config(bytes).unwrap();assert_eq!(fs::read(path.join("batting_config.json")).unwrap(),bytes);
        let mut schedule=crate::domain::test_support::game_schedule();schedule.max_inning=1;
        let case=run.begin(&schedule).unwrap();assert_eq!(case.batting_settings.decision(true,1).selected_intent,BattedBallIntent::ContactFirst);
        assert_eq!(case.batting_settings.decision(false,102).selected_intent,BattedBallIntent::LineDrive);
        assert!(run.freeze_batting_config(bytes).is_err());let result=simulate_case(&case).unwrap();
        run.saved(&result).unwrap();run.manifest.completed_rounds=1;run.complete().unwrap();
        assert_eq!(replay(&path,schedule.id).unwrap(),sha(&canonical(&result).unwrap()));
        let mut changed=bytes.to_vec();changed.push(b' ');fs::write(path.join("batting_config.json"),changed).unwrap();
        assert!(replay(&path,schedule.id).unwrap_err().to_string().contains("configuration hash mismatch"));
        fs::remove_dir_all(path).unwrap();
    }
    #[test]
    fn mixed_profile_is_id_only_and_preserves_both_levels() {
        let mut schedule=crate::domain::test_support::game_schedule();
        for team_id in 1..=3 {
            schedule.away_team.id=team_id;
            let settings=settings_for("mixed-id-v1",&schedule).unwrap();
            for id in [1,2] {
                let decision=settings.decision(true,id);
                assert_eq!(decision.preferences.preferred_intent,if id%2==0 {BattedBallIntent::ContactFirst}else{BattedBallIntent::LineDrive});
                assert_eq!(decision.selected_intent,match team_id%3 {0=>decision.preferences.preferred_intent,1=>BattedBallIntent::ContactFirst,_=>BattedBallIntent::LineDrive});
            }
            schedule.away_team.players.reverse();
            assert_eq!(canonical(&settings).unwrap(),canonical(&settings_for("mixed-id-v1",&schedule).unwrap()).unwrap());
        }
        assert!(settings_for("unknown",&schedule).is_err());
        schedule.home_team.players[0].info.id=schedule.away_team.players[0].info.id;
        assert!(settings_for("mixed-id-v1",&schedule).is_err());
    }
    #[test]
    fn mixed_case_replays_and_does_not_change_live_results_or_rng() {
        let path=folder();let mut run=RunRecorder::start(path.clone(),2026110503,1,0,2000,"test.db".into(),"en-US".into()).unwrap();
        run.set_batting_profile("mixed-id-v1").unwrap();
        let mut schedule=crate::domain::test_support::game_schedule();schedule.max_inning=1;
        let case=run.begin(&schedule).unwrap();assert!(run.set_batting_profile("default").is_err());
        let mixed=simulate_case(&case).unwrap();
        assert!(mixed.player_contact_states.iter().any(|r|r.state.is_some()));
        for r in &mixed.player_contact_states {
            if let Some(state)=&r.state {
                let context=state.intent_batter.as_ref().unwrap();
                let team=if context.is_top {&schedule.away_team}else{&schedule.home_team};
                assert!(team.players.iter().any(|p|p.info.id==context.batter_id));
                if let Some(observed)=&state.observed_intent_plans {
                    assert_eq!(canonical(&observed.decision).unwrap(),canonical(&case.batting_settings.decision(context.is_top,context.batter_id)).unwrap());
                }
            }
        }
        let mut baseline=case.clone();baseline.batting_settings=BattingStrategySettings::default();
        let mut a=serde_json::to_value(&mixed).unwrap();let mut b=serde_json::to_value(simulate_case(&baseline).unwrap()).unwrap();
        a.as_object_mut().unwrap().remove("player_contact_states");b.as_object_mut().unwrap().remove("player_contact_states");
        assert_eq!(a,b);
        run.saved(&mixed).unwrap();run.manifest.completed_rounds=1;run.complete().unwrap();
        assert_eq!(replay(&path,schedule.id).unwrap(),sha(&canonical(&mixed).unwrap()));
        fs::remove_dir_all(path).unwrap();
    }
    fn folder()->PathBuf {
        let id=std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        std::env::temp_dir().join(format!("saberbb-seeded-{}-{id}",std::process::id()))
    }
    #[test]
    fn frozen_case_replays_same_full_result_and_contacts() {
        let path=folder();
        let mut run=RunRecorder::start(path.clone(),2026110501,1,0,2000,"test.db".into(),"en-US".into()).unwrap();
        let mut schedule=crate::domain::test_support::game_schedule();schedule.max_inning=1;
        let case=run.begin(&schedule).unwrap();
        let a=simulate_case(&case).unwrap();let b=simulate_case(&case).unwrap();
        assert_eq!(sha(&canonical(&a).unwrap()),sha(&canonical(&b).unwrap()));
        run.saved(&a).unwrap();run.manifest.completed_rounds=1;run.complete().unwrap();
        assert_eq!(replay(&path,schedule.id).unwrap(),sha(&canonical(&a).unwrap()));
        fs::remove_dir_all(path).unwrap();
    }
    #[test]
    fn prepared_inputs_are_immutable_and_overlapping_or_duplicate_games_rejected() {
        let path=folder();let mut run=RunRecorder::start(path.clone(),7,1,1,2000,"test.db".into(),"en-US".into()).unwrap();
        let mut schedule=crate::domain::test_support::game_schedule();
        assert!(run.begin(&schedule).is_err());schedule.id=2;
        let case=run.begin(&schedule).unwrap();assert_eq!(case.game_seed,derived_seed(7,2).to_string());
        assert!(run.begin(&schedule).is_err());assert!(run.complete().is_err());
        assert!(RunRecorder::start(path.clone(),7,1,1,2000,"test.db".into(),"en-US".into()).is_err());
        assert_ne!(derived_seed(7,2),derived_seed(8,2));assert_ne!(derived_seed(7,2),derived_seed(7,3));
        run.fail("synthetic failure").unwrap();assert_eq!(run.manifest.status,"failed");assert!(replay(&path,2).is_err());
        fs::remove_dir_all(path).unwrap();
    }
    #[test]
    fn seed_and_contact_hash_match_cross_language_contract() {
        assert_eq!(derived_seed(2026110501,1),3257385400538808217);
        assert_eq!(derived_seed(7,2),5628531699786173391);
        let record=ContactStateRecord {count_seq:7,state_version:1,state_source:"abc".into(),invalid_reason:None,state:None};
        assert_eq!(contacts_sha256(&[record]).unwrap(),"a748868b4a64c88d24f7946edcdf42184265176a310daa18f3bc3dc96091ebc8");
    }
    struct Repo { schedules:Vec<GameSchedule>,saved:usize,updated:usize,fail:bool,folder:PathBuf }
    impl crate::repositories::game_repository::GameScheduleReader for Repo {
        fn load_game_schedules_to_process(&self)->Result<Vec<GameSchedule>,crate::error::AppError> { Ok(self.schedules.clone()) }
        fn load_team_game_schedules(&self,_id:u16,_start:chrono::NaiveDate,_end:chrono::NaiveDate)
            ->Result<Vec<crate::domain::shared::game::TeamGameScheduleView>,crate::error::AppError> { Ok(Vec::new()) }
    }
    impl crate::repositories::game_repository::GameResultWriter for Repo {
        fn update_game_result(&mut self,_game:&GameResult)->Result<(),crate::error::AppError> {
            assert_eq!(fs::read_dir(self.folder.join("cases")).unwrap().count(),self.schedules.len());
            if self.fail { return Err(crate::error::AppError::Internal(anyhow::anyhow!("synthetic save failure"))); }
            self.saved+=1;Ok(())
        }
    }
    impl crate::repositories::game_repository::GameRoundWriter for Repo {
        fn update_current_round_seq(&mut self)->Result<usize,crate::error::AppError> { self.updated+=1;Ok(1) }
    }
    #[test]
    fn changed_schedule_is_rejected_before_generating_or_saving_results() {
        let path=folder();let schedule=crate::domain::test_support::game_schedule();
        let mut repo=Repo {schedules:vec![schedule.clone()],saved:0,updated:0,fail:false,folder:path.clone()};
        let mut run=RunRecorder::start(path.clone(),31,1,0,2000,"test.db".into(),"en-US".into()).unwrap();
        assert!(run.set_round_plan(vec![PlannedRound {season:schedule.season,round_seq:schedule.round_seq,game_ids:vec![]}]).is_err());
        run.set_round_plan(vec![PlannedRound {season:schedule.season,round_seq:schedule.round_seq,game_ids:vec![schedule.id+1]}]).unwrap();
        assert!(process_run(&mut repo,&mut run).unwrap_err().to_string().contains("schedule changed"));
        assert_eq!((repo.saved,repo.updated),(0,0));assert!(run.manifest.games.is_empty());
        assert_eq!(run.manifest.status,"failed");fs::remove_dir_all(path).unwrap();
    }
    #[test]
    fn complete_and_failed_runs_never_silently_drop_games() {
        for fail in [false,true] {
            let path=folder();let mut schedule=crate::domain::test_support::game_schedule();schedule.max_inning=1;
            let mut second=schedule.clone();second.id=2;
            let mut repo=Repo {schedules:vec![schedule,second],saved:0,updated:0,fail,folder:path.clone()};
            let mut run=RunRecorder::start(path.clone(),31,1,0,2000,"test.db".into(),"en-US".into()).unwrap();
            run.set_round_plan(vec![PlannedRound {season:repo.schedules[0].season,round_seq:repo.schedules[0].round_seq,game_ids:vec![1,2]}]).unwrap();
            let result=process_run(&mut repo,&mut run);
            if fail { assert!(result.is_err());assert_eq!(run.manifest.status,"failed");assert_eq!(repo.updated,0); }
            else { assert!(result.is_ok(),"{result:?}");assert_eq!(run.manifest.status,"complete");assert_eq!(repo.saved,2);assert_eq!(repo.updated,1); }
            fs::remove_dir_all(path).unwrap();
        }
    }
}
