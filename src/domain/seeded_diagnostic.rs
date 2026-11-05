//! Explicit-seed diagnostic runs. Frozen pre-game inputs, replay, and DB-log provenance.
use crate::domain::random_provider::RealRng;
use crate::domain::shared::game::{GameResult,GameSchedule};
use crate::domain::shared::contact_state::ContactStateRecord;
use crate::domain::strategy::batting_strategy::BattingStrategySettings;
use crate::repositories::game_repository::GameRoundRepository;
use anyhow::{Result,Context,bail,ensure};
use serde::{Serialize,Deserialize};
use sha2::{Digest,Sha256};
use std::{fs::{self,OpenOptions},io::Write,path::{Path,PathBuf}};

pub const ENGINE: &str=env!("SABERBB_ENGINE_SOURCE_SHA256");
pub const BUILD: &str=env!("SABERBB_BUILD_SHA256");
pub const SOURCE: &str="seeded_intent_holdout_v1";
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
pub struct RunManifest {
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
    pub error:Option<String>,
}
pub struct RunRecorder { pub manifest:RunManifest, pub output:PathBuf }
impl RunRecorder {
    pub fn start(output:PathBuf,seed:u64,rounds:u32,exclude:u32,max_pitches:u32,db_path:PathBuf,language:String)->Result<Self> {
        ensure!(rounds>0&&max_pitches>0&&max_pitches<65535,"invalid run rounds or pitch budget");
        fs::create_dir(&output).context("output directory must be new (no overwrite)")?;
        fs::create_dir(output.join("cases"))?;
        let recorder=Self { output,manifest:RunManifest { version:1,source:SOURCE.into(),status:"running".into(),
            master_seed:seed.to_string(),seed_derivation:"SHA256(saberbb:holdout:v1\\0 + u64le(master) + u32le(game)); first 8 bytes u64le".into(),
            engine_source_sha256:ENGINE.into(),build_sha256:BUILD.into(),build_target:env!("SABERBB_BUILD_TARGET").into(),
            build_profile:env!("SABERBB_BUILD_PROFILE").into(),compiler:env!("SABERBB_BUILD_COMPILER").into(),
            git_commit:env!("SABERBB_GIT_COMMIT").into(),git_dirty:env!("SABERBB_GIT_DIRTY")=="true",cargo_lock_sha256:sha(include_bytes!("../../Cargo.lock")),
            model:crate::domain::shared::finite_bat_collision::MODEL.into(),
            frozen_policy_sha256:sha(include_bytes!("../../tools/diagnostics/pre_swing_aim_seed2_frozen.json")),source_db_path:db_path,language,
            requested_rounds:rounds,completed_rounds:0,exclude_through_game_id:exclude,max_pitches,
            selection:"next requested scheduled rounds; every unprocessed game, no outcome-based filtering".into(),games:Vec::new(),error:None } };
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
            schedule:schedule.clone(),batting_settings:BattingStrategySettings::default(),max_pitches:self.manifest.max_pitches };
        let file=format!("cases/game_{}.json",schedule.id);
        let hash=fresh_json(&self.output.join(&file),&case)?;
        self.manifest.games.push(CaseRecord { game_id:schedule.id,game_seed:seed.to_string(),input_file:file,input_sha256:hash,
            status:"prepared".into(),result_sha256:None,contacts_sha256:None,contact_states:None });
        self.save()?;
        // First execution and replay both consume the exact persisted JSON bytes.
        Ok(serde_json::from_slice(&fs::read(self.output.join(format!("cases/game_{}.json",schedule.id)))?)?)
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
        case.batting_settings.clone(),Some(case.max_pitches))
}

pub fn process_run<R:GameRoundRepository>(repo:&mut R,recorder:&mut RunRecorder)->Result<()> {
    let result=(||->Result<()> {
        for _ in 0..recorder.manifest.requested_rounds {
            let schedules=repo.load_game_schedules_to_process()?;
            ensure!(!schedules.is_empty(),"no scheduled games; no empty rounds accepted");
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
    fn complete_and_failed_runs_never_silently_drop_games() {
        for fail in [false,true] {
            let path=folder();let mut schedule=crate::domain::test_support::game_schedule();schedule.max_inning=1;
            let mut second=schedule.clone();second.id=2;
            let mut repo=Repo {schedules:vec![schedule,second],saved:0,updated:0,fail,folder:path.clone()};
            let mut run=RunRecorder::start(path.clone(),31,1,0,2000,"test.db".into(),"en-US".into()).unwrap();
            let result=process_run(&mut repo,&mut run);
            if fail { assert!(result.is_err());assert_eq!(run.manifest.status,"failed");assert_eq!(repo.updated,0); }
            else { assert!(result.is_ok(),"{result:?}");assert_eq!(run.manifest.status,"complete");assert_eq!(repo.saved,2);assert_eq!(repo.updated,1); }
            fs::remove_dir_all(path).unwrap();
        }
    }
}
