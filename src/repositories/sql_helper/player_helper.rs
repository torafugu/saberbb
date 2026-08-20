use crate::domain::shared::player::{
    ArmSlot, BatterInfo, BatterType, DefenseSkills, FielderInfo, FielderType, FullName,
    HitterTendency, PitchSkill, PitchType, PitcherInfo, PitcherStyle, PlayerInfo, Position, RL,
    RunningSkills,
};
use crate::error::AppError;
use crate::repositories::db::FromRow;
use rusqlite::types::{FromSql, FromSqlResult, ToSql, ToSqlOutput, ValueRef};
use validator::Validate;

impl ToSql for Position {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(ToSqlOutput::from(self.as_ref()))
    }
}

impl FromSql for Position {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        let gt = value.as_str()?;

        gt.parse::<Position>().map_err(|e| {
            eprintln!("{} {}: {:?}", "Parse error at ", gt, e);
            rusqlite::types::FromSqlError::InvalidType
        })
    }
}

impl ToSql for FielderType {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(ToSqlOutput::from(self.as_ref()))
    }
}

impl FromSql for FielderType {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        let gt = value.as_str()?;

        gt.parse::<FielderType>().map_err(|e| {
            eprintln!("{} {}: {:?}", "Parse error at ", gt, e);
            rusqlite::types::FromSqlError::InvalidType
        })
    }
}

impl ToSql for RL {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(ToSqlOutput::from(self.as_ref()))
    }
}

impl FromSql for RL {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        let gt = value.as_str()?;

        gt.parse::<RL>().map_err(|e| {
            eprintln!("{} {}: {:?}", "Parse error at ", gt, e);
            rusqlite::types::FromSqlError::InvalidType
        })
    }
}

impl ToSql for ArmSlot {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(ToSqlOutput::from(self.as_ref()))
    }
}

impl FromSql for ArmSlot {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        let gt = value.as_str()?;

        gt.parse::<ArmSlot>().map_err(|e| {
            eprintln!("{} {}: {:?}", "Parse error at ", gt, e);
            rusqlite::types::FromSqlError::InvalidType
        })
    }
}

impl FromRow for FullName {
    type Error = AppError;

    fn from_row(row: &rusqlite::Row) -> Result<Self, Self::Error> {
        let full_name = FullName {
            first: row.get("first_name")?,
            last: row.get("last_name")?,
        };

        full_name.validate()?;

        Ok(full_name)
    }
}

impl ToSql for PitchType {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(ToSqlOutput::from(self.as_ref()))
    }
}

impl FromSql for PitchType {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        let gt = value.as_str()?;

        gt.parse::<PitchType>().map_err(|e| {
            eprintln!("{} {}: {:?}", "Parse error at ", gt, e);
            rusqlite::types::FromSqlError::InvalidType
        })
    }
}

impl FromSql for PitcherStyle {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        let gt = value.as_str()?;

        gt.parse::<PitcherStyle>().map_err(|e| {
            eprintln!("{} {}: {:?}", "Parse error at", gt, e);
            rusqlite::types::FromSqlError::InvalidType
        })
    }
}

impl ToSql for PitcherStyle {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(ToSqlOutput::from(self.as_ref()))
    }
}

impl FromRow for PlayerInfo {
    type Error = AppError;

    fn from_row(row: &rusqlite::Row) -> Result<Self, Self::Error> {
        let player = PlayerInfo::new(
            row.get("id")?,
            row.get("first_name")?,
            row.get("last_name")?,
            row.get("age")?,
            row.get("uniform_number")?,
        );

        player.validate()?;

        Ok(player)
    }
}

impl FromRow for DefenseSkills {
    type Error = AppError;

    fn from_row(row: &rusqlite::Row) -> Result<Self, Self::Error> {
        let defensive_skills = DefenseSkills {
            position: row.get("position")?,
            pitcher: None,
            catcher: None,
            middle_infielder: None,
            corner_infielder: None,
            outfielder: None,
        };

        defensive_skills.validate()?;

        Ok(defensive_skills)
    }
}

impl FromRow for FielderInfo {
    type Error = AppError;

    fn from_row(row: &rusqlite::Row) -> Result<Self, Self::Error> {
        let fielder_info = FielderInfo {
            fielder_type: row.get("fielder_type")?,
            throw_speed: row.get("throw_speed")?,
            running_speed: row.get("running_speed")?,
            reaction: row.get("reaction")?,
            prep_time: row.get("prep_time")?,
            catching: row.get("catching")?,
            reach_height: row.get("reach_height")?,
            reach_range: row.get("reach_range")?,
        };

        fielder_info.validate()?;

        Ok(fielder_info)
    }
}

impl FromRow for PitcherInfo {
    type Error = AppError;

    fn from_row(row: &rusqlite::Row) -> Result<Self, Self::Error> {
        let pitcher_info = PitcherInfo {
            height: row.get("height")?,
            extension: row.get("extension")?,
            throw_side: row.get("throw_side")?,
            arm_slot: row.get("arm_slot")?,
            pitcher_style: row.get("pitcher_style")?,
            velocity: row.get("velocity")?,
            spin_rate: row.get("spin_rate")?,
            control: row.get("control")?,
            stamina: row.get("stamina")?,
            injury_proneness: row.get("injury_proneness")?,
            clutch: row.get("clutch")?,
            hpp: row.get("hpp")?,
            platoon_splitting: row.get("platoon_splitting")?,
            delivery_motion_time: row.get("delivery_motion_time")?,
            pitch_skills: Vec::new(),
            fielder_info: FielderInfo::new_pitcher(),
        };

        pitcher_info.validate()?;

        Ok(pitcher_info)
    }
}

impl FromRow for PitchSkill {
    type Error = AppError;

    fn from_row(row: &rusqlite::Row) -> Result<Self, Self::Error> {
        let pitch_skill = PitchSkill {
            pitch_type: row.get("pitch_type")?,
            velocity: row.get("velocity")?,
            control: row.get("control")?,
            stamina: row.get("stamina")?,
            injury_proneness: row.get("injury_proneness")?,
            spin_rate: row.get("spin_rate")?,
            spin_angle: row.get("spin_angle")?,
            spin_efficiency: row.get("spin_efficiency")?,
            usage: row.get("usage")?,
        };

        pitch_skill.validate()?;

        Ok(pitch_skill)
    }
}

impl FromSql for HitterTendency {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        let gt = value.as_str()?;

        gt.parse::<HitterTendency>().map_err(|e| {
            eprintln!("{} {}: {:?}", "Parse error at", gt, e);
            rusqlite::types::FromSqlError::InvalidType
        })
    }
}

impl ToSql for HitterTendency {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(ToSqlOutput::from(self.as_ref()))
    }
}

impl FromSql for BatterType {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        let gt = value.as_str()?;

        gt.parse::<BatterType>().map_err(|e| {
            eprintln!("{} {}: {:?}", "Parse error at", gt, e);
            rusqlite::types::FromSqlError::InvalidType
        })
    }
}

impl ToSql for BatterType {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(ToSqlOutput::from(self.as_ref()))
    }
}

impl FromRow for BatterInfo {
    type Error = AppError;

    fn from_row(row: &rusqlite::Row) -> Result<Self, Self::Error> {
        let batter_info = BatterInfo {
            batting_side: row.get("batting_side")?,
            batter_type: row.get("batter_type")?,
            batting_eye: row.get("batting_eye")?,
            swing_speed: row.get("swing_speed")?,
            swing_power: row.get("swing_power")?,
            attack_angle: row.get("attack_angle")?,
            bat_control: row.get("bat_control")?,
            timing_bias: row.get("timing_bias")?,
            consistency_sigma: row.get("consistency_sigma")?,
        };

        batter_info.validate()?;

        Ok(batter_info)
    }
}

impl FromRow for RunningSkills {
    type Error = AppError;

    fn from_row(row: &rusqlite::Row) -> Result<Self, Self::Error> {
        let running_skills = RunningSkills {
            speed: row.get("speed")?,
            lead_distance: row.get("lead_distance")?,
            start_reaction: row.get("start_reaction")?,
        };

        running_skills.validate()?;

        Ok(running_skills)
    }
}
