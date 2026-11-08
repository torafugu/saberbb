use super::locale::tr;
use dioxus::prelude::*;
use crate::application::viewing::{self, Catalog};
use super::{blocking, tables::{DataTable, ErrorPanel}, game::Games, teams::Teams};

#[component]
pub fn App() -> Element {
    let language=use_signal(super::locale::Language::current);
    use_context_provider(||language);
    let mut refresh = use_signal(|| 0u32);
    super::management::provide_controller(refresh);
    let catalog = use_resource(move || { refresh(); blocking(viewing::catalog) });
    rsx! {
        style { {include_str!("style.css")} }
        div { class: "app-shell",
            match catalog.read().as_ref() {
                None => rsx! { main { class:"loading", {tr("データベースを読み込み中…")} } },
                Some(Err(e)) => rsx! { main { ErrorPanel {message:e.clone()} button {onclick:move |_|refresh+=1,{tr("再読み込み")}} } },
                Some(Ok(data)) => rsx! { Browser { catalog:data.clone(), refresh:refresh(), onrefresh:move |_|refresh+=1 } },
            }
        }
    }
}

#[component]
pub(super) fn Browser(catalog: Catalog, refresh:u32, onrefresh: EventHandler<()>) -> Element {
    let mut view = use_signal(|| 0u8);
    let mut league = use_signal(|| 0u16);
    let mut team = use_signal(|| 0u16);
    let mut season = use_signal(|| 0u16);
    let locale=super::locale::language();
    let stats_team=if view()==1 {0} else {team()};
    let controller=use_context::<super::management::Controller>();
    let busy=controller.state.read().busy;
    let headings = [{tr("試合結果")}, {tr("順位表")}, {tr("打撃成績")}, {tr("投手成績")}, {tr("チーム・選手")}, {tr("管理")}];
    let labels = ["Games", "Standings", "Batting", "Pitching", "Teams", "Manage"];
    rsx! {
        aside { class:"sidebar",
            div { class:"brand", span { class:"brand-mark","S" } div { strong {"SABERBB"} small {"BASEBALL LAB"} } }
            p { class:"nav-label","BROWSE" }
            nav {
                for (i,label) in headings.iter().enumerate() {
                    button { id:format!("nav-{i}"), class:if view()==i as u8 {"nav-item selected"} else {"nav-item"},
                        onclick:move |_|view.set(i as u8), span {"{labels[i]}"} strong {"{label}"} }
                }
            }
            div { class:"sidebar-note", {tr("閲覧も管理もこの画面から")} }
        }
        main {
            header { class:"topbar", div { p {class:"eyebrow","SEASON ARCHIVE / LIVE DATABASE"} h1 {"{headings[view() as usize]}"} }
                super::locale::LanguageSwitch {disabled:busy}
                button {id:format!("refresh"),class:"refresh",disabled:busy,onclick:move |_|onrefresh.call(()),{tr("↻ 再読み込み")}} }
            if busy { button {class:"job-banner",onclick:move |_|view.set(5),{tr("管理処理を実行中… 進捗を見る")}} }
            if view()!=5 {
            div { class:"filters",
                label { {tr("リーグ")} select { id:format!("league-filter"), value:league().to_string(), onchange:move |e|{league.set(e.value().parse().unwrap_or(0));team.set(0);},
                    option { value:"0",{tr("全リーグ")} }
                    for l in &catalog.leagues { option {value:l.id.to_string(),"{l.name}"} }
                } }
                if view()==1 { label { {tr("シーズン")} select { id:format!("standings-season-filter"), value:season().to_string(), onchange:move |e|season.set(e.value().parse().unwrap_or(0)),
                    option {value:"0",selected:season()==0,{tr("全シーズン")}}
                    for year in &catalog.standing_seasons { option {value:year.to_string(),selected:season()==*year,"{year}"} }
                } } }
                if view()!=1 { label { {tr("チーム")} select { id:format!("team-filter"), value:team().to_string(), onchange:move |e|team.set(e.value().parse().unwrap_or(0)),
                    option {value:"0",{tr("全チーム")}}
                    for t in catalog.teams.iter().filter(|t|league()==0 || t.league==league()) { option {value:t.id.to_string(),"{t.name}"} }
                } }
                }
                span { class:"scope-note", if view()==1 && season()!=0 {{tr("集計範囲：選択シーズン")}} else if (1..=3).contains(&view()) {{tr("集計範囲：保存済み全試合（通算）")}} else {{tr("同じローカルDBから表示")}} }
            }
            }
            match view() {
                0 => rsx!{ Games {key:"games-{league()}-{team()}-{refresh}",catalog:catalog.clone(),league:league(),team:team(),refresh} },
                1..=3 => rsx!{ Statistics {key:"stats-{view()}-{league()}-{stats_team}-{season()}-{locale:?}",kind:view(),catalog:catalog.clone(),league:league(),team:stats_team,season:if view()==1 {season()} else {0},refresh} },
                4 => rsx!{ Teams {key:"team-{team()}",team:team(),refresh} },
                _ => rsx!{ super::management::Management {} },
            }
        }
    }
}

#[component]
fn Statistics(kind:u8, catalog:Catalog, league:u16, team:u16, season:u16, refresh:u32) -> Element {
    let data = use_resource(use_reactive((&kind,&league,&team,&season,&refresh,&catalog), |(kind,league,team,season,_refresh,catalog)| async move {
        blocking(move || if kind==1 {viewing::season_standings(season,league,&catalog)} else {viewing::statistics(kind,league,team,&catalog)}).await
    }));
    rsx! { match data.read().as_ref() {
        None=>rsx!{p {class:"loading",{tr("成績を読み込み中…（初回のみ集計します）")}}},
        Some(Err(e))=>rsx!{ErrorPanel {message:e.clone()}},
        Some(Ok(table))=>rsx!{
            if kind==1 {p {class:"small-note",{tr("防御率は失点と記録アウト数による参考値です（自責点は未記録）。")}}}
            DataTable {data:table.clone(),searchable:kind!=1,paginated:kind!=1,initial_sort:if kind==1 {None}else{Some((1,true))}}},
    } }
}
