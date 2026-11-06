//! Persistent people and pre-pitch decision inputs. Coefficients are diagnostic hypotheses.
use serde::{Serialize,Deserialize};
use crate::domain::strategy::batting_strategy::{BattingStrategy,BattedBallIntent,BattingPreferences,BattingIntentDecision,decide_batting_intent};

#[derive(Clone,Copy,Debug,Default,PartialEq,Eq,Serialize,Deserialize)]
#[serde(rename_all="snake_case")]
pub enum BatterCharacter { ContactOriented,LineDriveOriented,Adaptive,#[default] Balanced }
#[derive(Clone,Copy,Debug,PartialEq,Serialize,Deserialize)]
pub struct BatterTendencies { pub contact_priority:f64,pub line_drive_priority:f64,pub adaptability:f64 }
impl BatterCharacter {
    pub fn tendencies(self)->BatterTendencies {
        let (contact_priority,line_drive_priority,adaptability)=match self {
            Self::ContactOriented=>(0.8,0.2,0.4),Self::LineDriveOriented=>(0.2,0.8,0.4),
            Self::Adaptive=>(0.5,0.5,1.0),Self::Balanced=>(0.5,0.5,0.5),
        };BatterTendencies {contact_priority,line_drive_priority,adaptability}
    }
}
#[derive(Clone,Copy,Debug,PartialEq,Eq,Serialize,Deserialize)]
pub struct BattingSituation { pub balls:u8,pub strikes:u8,pub outs:u8,pub runners:[bool;3] }
impl BattingSituation {
    pub fn validate(&self)->Result<(),&'static str> {
        if self.balls>3||self.strikes>2||self.outs>2 {Err("invalid_pre_pitch_batting_situation")}else{Ok(())}
    }
}
#[derive(Clone,Copy,Debug,PartialEq,Serialize,Deserialize)]
pub struct SituationalBattingPreferences {
    pub contact_weight:f64,pub line_drive_weight:f64,pub preferred_intent:BattedBallIntent,
    pub two_strike_protection:bool,pub runner_advancement:bool,pub production_opportunity:bool,
}
pub fn generate_preferences(character:BatterCharacter,situation:BattingSituation)->Result<SituationalBattingPreferences,&'static str> {
    situation.validate()?;let t=character.tendencies();
    let protect=situation.strikes==2;
    let advance=situation.runners[2]&&situation.outs<2;
    let produce=(situation.balls>=2&&situation.strikes==0)||((situation.runners[1]||situation.runners[2])&&situation.outs==2);
    let contact=t.contact_priority+t.adaptability*(if protect {0.5}else{0.0}+if advance {0.25}else{0.0});
    let line=t.line_drive_priority+t.adaptability*if produce {0.25}else{0.0};let sum=contact+line;
    Ok(SituationalBattingPreferences {contact_weight:contact/sum,line_drive_weight:line/sum,
        preferred_intent:if line>contact {BattedBallIntent::LineDrive}else{BattedBallIntent::ContactFirst},
        two_strike_protection:protect,runner_advancement:advance,production_opportunity:produce})
}
#[derive(Clone,Copy,Debug,Default,PartialEq,Eq,Serialize,Deserialize)]
#[serde(rename_all="snake_case")]
pub enum ManagerBattingPolicy { #[default] LetBatterDecide,PutBallInPlay,SeekLineDrive,Situational }
#[derive(Clone,Debug,PartialEq,Eq,Serialize,Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manager { pub id:i64,pub name:String,pub batting_policy:ManagerBattingPolicy }
#[derive(Clone,Copy,Debug,PartialEq,Eq,Serialize,Deserialize)]
#[serde(rename_all="snake_case")]
pub enum ManagerInstructionReason { NoManager,StandingOrder,BatterDiscretion,TwoStrikeProtection,RunnerAdvancement,RunProduction }
#[derive(Clone,Copy,Debug,PartialEq,Eq,Serialize,Deserialize)]
pub struct ManagerInstruction { pub strategy:BattingStrategy,pub reason:ManagerInstructionReason }
impl Manager {
    pub fn validate(&self)->Result<(),&'static str> {if self.id<=0||self.name.trim().is_empty(){Err("invalid_manager_identity")}else{Ok(())}}
    pub fn instruction(&self,s:BattingSituation)->Result<ManagerInstruction,&'static str> {
        self.validate()?;s.validate()?;
        use ManagerInstructionReason::*;
        let (strategy,reason)=match self.batting_policy {
            ManagerBattingPolicy::LetBatterDecide=>(BattingStrategy::LetBatterDecide,StandingOrder),
            ManagerBattingPolicy::PutBallInPlay=>(BattingStrategy::PutBallInPlay,StandingOrder),
            ManagerBattingPolicy::SeekLineDrive=>(BattingStrategy::SeekLineDrive,StandingOrder),
            ManagerBattingPolicy::Situational if s.strikes==2=>(BattingStrategy::PutBallInPlay,TwoStrikeProtection),
            ManagerBattingPolicy::Situational if s.runners[2]&&s.outs<2=>(BattingStrategy::PutBallInPlay,RunnerAdvancement),
            ManagerBattingPolicy::Situational if (s.balls>=2&&s.strikes==0)||((s.runners[1]||s.runners[2])&&s.outs==2)=>(BattingStrategy::SeekLineDrive,RunProduction),
            ManagerBattingPolicy::Situational=>(BattingStrategy::LetBatterDecide,BatterDiscretion),
        };Ok(ManagerInstruction {strategy,reason})
    }
}
#[derive(Clone,Debug,Serialize,Deserialize)]
pub struct BattingSelectionTrace {
    pub version:u8,pub source:String,pub batter_id:i64,pub character:BatterCharacter,
    pub tendencies:BatterTendencies,pub situation:BattingSituation,pub preferences:SituationalBattingPreferences,
    pub manager:Option<Manager>,pub instruction:ManagerInstruction,pub decision:BattingIntentDecision,
}
pub fn select_batting_intent(batter_id:i64,character:BatterCharacter,situation:BattingSituation,manager:Option<&Manager>)->Result<BattingSelectionTrace,&'static str> {
    if batter_id<=0 {return Err("invalid_batter_identity");}
    let preferences=generate_preferences(character,situation)?;
    let instruction=match manager {Some(m)=>m.instruction(situation)?,None=>ManagerInstruction {strategy:BattingStrategy::LetBatterDecide,reason:ManagerInstructionReason::NoManager}};
    let decision=decide_batting_intent(instruction.strategy,BattingPreferences {preferred_intent:preferences.preferred_intent});
    Ok(BattingSelectionTrace {version:1,source:"player_manager_batting_v1".into(),batter_id,character,tendencies:character.tendencies(),situation,preferences,
        manager:manager.cloned(),instruction,decision})
}

#[cfg(test)]
mod tests {
    use super::*;
    fn situation()->BattingSituation {BattingSituation {balls:0,strikes:0,outs:0,runners:[false;3]}}
    #[test]
    fn character_and_situation_change_preferences_before_manager_intervention() {
        let s=situation();assert_eq!(generate_preferences(BatterCharacter::ContactOriented,s).unwrap().preferred_intent,BattedBallIntent::ContactFirst);
        assert_eq!(generate_preferences(BatterCharacter::LineDriveOriented,s).unwrap().preferred_intent,BattedBallIntent::LineDrive);
        let mut hitter=s;hitter.balls=2;let mut protect=hitter;protect.strikes=2;
        assert_eq!(generate_preferences(BatterCharacter::Adaptive,hitter).unwrap().preferred_intent,BattedBallIntent::LineDrive);
        assert_eq!(generate_preferences(BatterCharacter::Adaptive,protect).unwrap().preferred_intent,BattedBallIntent::ContactFirst);
        let both=generate_preferences(BatterCharacter::Adaptive,BattingSituation {strikes:2,runners:[false,false,true],..s}).unwrap();
        assert!((both.contact_weight-5.0/7.0).abs()<1e-12);
        let manager=Manager {id:42,name:"Coach".into(),batting_policy:ManagerBattingPolicy::PutBallInPlay};
        let trace=select_batting_intent(7,BatterCharacter::LineDriveOriented,hitter,Some(&manager)).unwrap();
        assert_eq!(trace.preferences.preferred_intent,BattedBallIntent::LineDrive);assert_eq!(trace.decision.selected_intent,BattedBallIntent::ContactFirst);
        assert_eq!(trace.manager.as_ref().unwrap().id,42);
        assert_eq!(trace.instruction.reason,ManagerInstructionReason::StandingOrder);
        assert!(generate_preferences(BatterCharacter::Balanced,BattingSituation {strikes:3,..s}).is_err());
    }
    #[test]
    fn independent_manager_changes_instruction_with_situation() {
        let manager=Manager {id:9,name:"Manager".into(),batting_policy:ManagerBattingPolicy::Situational};
        let s=situation();assert_eq!(manager.instruction(s).unwrap().strategy,BattingStrategy::LetBatterDecide);
        assert_eq!(manager.instruction(BattingSituation {strikes:2,..s}).unwrap().reason,ManagerInstructionReason::TwoStrikeProtection);
        assert_eq!(manager.instruction(BattingSituation {runners:[false,false,true],..s}).unwrap().reason,ManagerInstructionReason::RunnerAdvancement);
        assert_eq!(manager.instruction(BattingSituation {balls:2,..s}).unwrap().strategy,BattingStrategy::SeekLineDrive);
    }
}

#[cfg(test)]
mod contract_tests {
    use super::*;
    fn same(a:&serde_json::Value,b:&serde_json::Value) {
        match (a,b) {
            (serde_json::Value::Object(a),serde_json::Value::Object(b))=>{assert_eq!(a.len(),b.len());for (k,v) in a {same(v,&b[k]);}},
            (serde_json::Value::Array(a),serde_json::Value::Array(b))=>{assert_eq!(a.len(),b.len());for (a,b) in a.iter().zip(b){same(a,b);}},
            (serde_json::Value::Number(a),serde_json::Value::Number(b))=>assert!((a.as_f64().unwrap()-b.as_f64().unwrap()).abs()<1e-10),
            _=>assert_eq!(a,b),
        }
    }
    #[test]
    fn player_manager_trace_matches_cross_language_contract() {
        let fixture:serde_json::Value=serde_json::from_str(include_str!("../../../tests/fixtures/player_manager_batting_contract.json")).unwrap();
        for case in fixture["cases"].as_array().unwrap() {
            let input=&case["input"];
            let character=serde_json::from_value(input["character"].clone()).unwrap();
            let situation=serde_json::from_value(input["situation"].clone()).unwrap();
            let manager:Option<Manager>=serde_json::from_value(input["manager"].clone()).unwrap();
            let trace=select_batting_intent(7,character,situation,manager.as_ref()).unwrap();
            same(&serde_json::to_value(trace).unwrap(),&case["expected"]);
        }
    }
}
