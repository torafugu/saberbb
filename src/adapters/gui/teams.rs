use dioxus::prelude::*;
use crate::application::viewing::{self, TableData};
use crate::domain::shared::player::Player;
use chrono::{Datelike, Local, Months, NaiveDate};
use super::{blocking, tables::{DataTable, ErrorPanel}, game::GamePanel};

#[component]
pub(super) fn Teams(team:u16, refresh:u32) -> Element {
    let mut tab=use_signal(||0u8);
    let mut selected=use_signal(||None::<i64>);
    let mut game=use_signal(||None::<u32>);
    let mut position=use_signal(||"All".to_owned());
    let roster=use_resource(use_reactive((&team,&refresh),|(team,_refresh)|async move {
        blocking(move || Ok((viewing::players(team)?,viewing::manager(team)?))).await
    }));
    if team==0 { return rsx!{p {class:"empty","上のフィルターからチームを選択してください。選手の能力・球種とチーム日程を表示します。"}}; }
    if let Some(id)=game() { return rsx!{GamePanel {key:"team-game-{id}-{refresh}",id,refresh,onclose:move |_|game.set(None)}}; }
    rsx!{
        div {class:"tabs",
            button {id:format!("team-tab-0"),class:if tab()==0 {"active"}else{""},onclick:move |_|tab.set(0),"選手・監督"}
            button {id:format!("team-tab-1"),class:if tab()==1 {"active"}else{""},onclick:move |_|tab.set(1),"チーム日程"}
        }
        if tab()==1 {
            TeamSchedule {key:"schedule-{team}-{refresh}",team,refresh,onopen:move |id|game.set(Some(id))}
        } else {
            match roster.read().as_ref() {
                None=>rsx!{p {class:"loading","選手を読み込み中…"}},
                Some(Err(e))=>rsx!{ErrorPanel {message:e.clone()}},
                Some(Ok((players,manager)))=>rsx!{
                    div {class:"manager",
                        if let Some(m)=manager {strong {"監督：{m.name}"} span {" / {m.batting_policy:?}"}}
                        else {"監督未配置（管理TUIのキャラクター作成で補充できます）"}
                    }
                    div {class:"toolbar",
                        label {"ポジション" select {value:position,onchange:move |e|position.set(e.value()),
                            option {value:"All","全選手"} option {value:"P","投手"} option {value:"C","捕手"}
                            option {value:"IF","内野・DH"} option {value:"OF","外野"}
                        }}
                    }
                    section {class:"panel", div {class:"table-scroll",table {
                        thead {tr {th {"選手"} th {"背番号"} th {"年齢"} th {"守備位置"} th {"打撃傾向"}}}
                        tbody {for p in players.iter().filter(|p|position_matches(p,&position())) {
                            tr {td {button {id:format!("roster-{}",p.info.id),class:"roster-button",onclick:{let id=p.info.id;move |_|selected.set(Some(id))},"{p.info.full_name()}"}}
                                td {"{p.info.uniform_number}"} td {"{p.info.age}"} td {"{p.defense_skills.position}"} td {"{p.batting_character:?}"}
                            }
                        }}
                    }}}
                    if let Some(p)=players.iter().find(|p|Some(p.info.id)==selected()) {
                        section {class:"panel player-detail",h2 {"{p.info.full_name()}"} p {class:"muted","#{p.info.uniform_number} · {p.info.age}歳 · {p.defense_skills.position}"}}
                        for (index, table) in player_tables(p).into_iter().enumerate() {
                            DataTable {key:"player-{p.info.id}-{index}",data:table,interactive:false}
                        }
                    }
                },
            }
        }
    }
}
fn position_matches(p:&Player, lane:&str)->bool {
    use crate::domain::shared::player::Position::*;
    let pos=p.defense_skills.position;
    match lane {"P"=>pos==P,"C"=>pos==C,"IF"=>matches!(pos,FB|SB|TB|SS|DH),"OF"=>pos.is_outfielder(),_=>true}
}
fn player_tables(player:&Player)->Vec<TableData> {
    let sections=[("打撃能力",serde_json::to_value(&player.offense_skills.batter).unwrap()),
        ("走塁能力",serde_json::to_value(player.offense_skills.running).unwrap()),
        ("守備・投球能力",serde_json::to_value(&player.defense_skills).unwrap())];
    sections.into_iter().filter(|(_,v)|!v.is_null()).map(|(title,value)| {
        let mut rows=Vec::new(); flatten("",&value,&mut rows);
        TableData::new(title,&["パラメータ","値"],rows)
    }).collect()
}
fn flatten(prefix:&str,value:&serde_json::Value,rows:&mut Vec<Vec<String>>) {
    match value {
        serde_json::Value::Object(map)=>for (key,v) in map { flatten(&if prefix.is_empty(){key.clone()}else{format!("{prefix} / {key}")},v,rows); },
        serde_json::Value::Array(items)=>for (i,v) in items.iter().enumerate() {flatten(&format!("{prefix} / {}",i+1),v,rows);},
        serde_json::Value::Null=>{},
        serde_json::Value::String(s)=>rows.push(vec![prefix.to_owned(),s.clone()]),
        other=>rows.push(vec![prefix.to_owned(),other.to_string()]),
    }
}

#[component]
fn TeamSchedule(team:u16,refresh:u32,onopen:EventHandler<u32>)->Element {
    let mut month=use_signal(||format!("{}-{:02}",Local::now().year(),Local::now().month()));
    let data=use_resource(move || {
        let month=month();
        async move { blocking(move || {
            let from=NaiveDate::parse_from_str(&format!("{month}-01"),"%Y-%m-%d")?;
            let to=from.checked_add_months(Months::new(1)).ok_or_else(||anyhow::anyhow!("invalid month"))?.pred_opt().unwrap();
            viewing::schedule(team,from,to)
        }).await }
    });
    // The parent key includes refresh; a manual refresh remounts this resource.
    let _=refresh;
    rsx!{
        div {class:"toolbar",label {"月" input {id:format!("schedule-month"),r#type:"month",value:month,onchange:move |e|month.set(e.value())}}}
        match data.read().as_ref() {
            None=>rsx!{p {class:"loading","日程を読み込み中…"}},Some(Err(e))=>rsx!{ErrorPanel {message:e.clone()}},
            Some(Ok(games))=>rsx!{
                if games.is_empty(){p {class:"empty","この月の日程はありません。"}}
                div {class:"game-list",for g in games {
                    button {class:"game-card",disabled:g.actual_date.is_none(),onclick:{let id=g.id;move |_|onopen.call(id)},
                        time {"{g.planned_date}"} strong {"{g.away_team.name}"}
                        span {class:"score",{format!("{} : {}", g.away_points.map(|v|v.to_string()).unwrap_or_else(||"—".into()), g.home_points.map(|v|v.to_string()).unwrap_or_else(||"—".into()))}}
                        strong {class:"home","{g.home_team.name}"} span {class:"tag",if g.actual_date.is_some(){"結果を見る"}else{"未実施"}}
                    }
                }}
            },
        }
    }
}
