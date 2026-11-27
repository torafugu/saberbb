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
    // The Games screen keeps its own season/search/page state so those filters can
    // share the League/Team row rendered here.
    let mut games_season = use_signal(|| catalog.seasons.first().copied().unwrap_or(0));
    let mut games_query = use_signal(String::new);
    let mut games_page = use_signal(|| 0usize);
    // Standings, Batting and Pitching each remember their own season and start on the
    // latest processed season, matching the Games list. 0 keeps the career total.
    let stats_default_season = catalog.seasons.last().copied().unwrap_or(0);
    let standings_season = use_signal(|| stats_default_season);
    let batting_season = use_signal(|| stats_default_season);
    let pitching_season = use_signal(|| stats_default_season);
    let season_filter = match view() {
        1 => Some((standings_season, "standings-season-filter")),
        2 => Some((batting_season, "batting-season-filter")),
        3 => Some((pitching_season, "pitching-season-filter")),
        _ => None,
    };
    let stats_season = match view() {
        2 => batting_season(),
        3 => pitching_season(),
        _ => standings_season(),
    };
    let qualified_batters = use_signal(|| false);
    let qualified_pitchers = use_signal(|| false);
    let qualification_filter = match view() {
        2 => Some((qualified_batters, "qualified-batting-filter", "規定打席以上のみ")),
        3 => Some((qualified_pitchers, "qualified-pitching-filter", "規定投球回以上のみ")),
        _ => None,
    };
    let qualified_only = qualification_filter.is_some_and(|(checked, _, _)| checked());
    let locale=super::locale::language();
    let stats_team=if view()==1 {0} else {team()};
    let controller=use_context::<super::management::Controller>();
    let busy=controller.state.read().busy;
    let headings = [{tr("試合結果")}, {tr("順位表")}, {tr("打撃成績")}, {tr("投手成績")}, {tr("チーム・選手")}, {tr("管理")}];
    rsx! {
        aside { class:"sidebar",
            div { class:"brand", span { class:"brand-mark","S" } div { strong {"SABERBB"} small {"BASEBALL LAB"} } }
            nav {
                for (i,label) in headings.iter().enumerate() {
                    button { id:format!("nav-{i}"), class:if view()==i as u8 {"nav-item selected"} else {"nav-item"},
                        onclick:move |_|view.set(i as u8), strong {"{label}"} }
                }
            }
        }
        main {
            header { class:"topbar", div { p {class:"eyebrow","SEASON ARCHIVE / LIVE DATABASE"} h1 {"{headings[view() as usize]}"} }
                // The language switch is only offered on the Management screen.
                if view()==5 { super::locale::LanguageSwitch {disabled:busy} }
                button {id:format!("refresh"),class:"refresh",disabled:busy,onclick:move |_|onrefresh.call(()),{tr("↻ 再読み込み")}} }
            if busy { button {class:"job-banner",onclick:move |_|view.set(5),{tr("管理処理を実行中… 進捗を見る")}} }
            if view()!=5 {
            div { class:"filters",
                if view()==0 { label { {tr("シーズン")} select { id:format!("season-filter"), value:games_season().to_string(), onchange:move |e|{games_season.set(e.value().parse().unwrap_or(0));games_page.set(0);},
                    if catalog.seasons.is_empty() { option {value:"0",{tr("完了試合なし")}} }
                    for year in &catalog.seasons { option {value:year.to_string(),selected:games_season()==*year,"{year}"} }
                } } }
                if let Some((mut selected, id)) = season_filter { label { {tr("シーズン")} select { id: id, value:selected().to_string(), onchange:move |e|selected.set(e.value().parse().unwrap_or(0)),
                    option {value:"0",selected:selected()==0,{tr("全シーズン")}}
                    for year in &catalog.seasons { option {value:year.to_string(),selected:selected()==*year,"{year}"} }
                } } }
                label { {tr("リーグ")} select { id:format!("league-filter"), value:league().to_string(), onchange:move |e|{league.set(e.value().parse().unwrap_or(0));team.set(0);},
                    option { value:"0",{tr("全リーグ")} }
                    for l in &catalog.leagues { option {value:l.id.to_string(),"{l.name}"} }
                } }
                if view()!=1 { label { {tr("チーム")} select { id:format!("team-filter"), value:team().to_string(), onchange:move |e|team.set(e.value().parse().unwrap_or(0)),
                    option {value:"0",{tr("全チーム")}}
                    for t in catalog.teams.iter().filter(|t|league()==0 || t.league==league()) { option {value:t.id.to_string(),"{t.name}"} }
                } }
                }
                if view()==0 {
                    input { id:format!("game-search"), class:"game-search", r#type:"search", "aria-label":tr("試合を検索"), placeholder:tr("日付・チームを検索…"), value:games_query,
                        oninput:move |e|{games_query.set(e.value());games_page.set(0);} }
                }
                if let Some((mut checked, id, label)) = qualification_filter {
                    label { class: "qualification-filter",
                        input { id: id, r#type: "checkbox", checked: checked(),
                            onchange: move |event| checked.set(event.checked())
                        }
                        {tr(label)}
                    }
                }
            }
            }
            match view() {
                0 => rsx!{ Games {
                    key:"games-{league()}-{team()}-{refresh}", catalog:catalog.clone(), league:league(), team:team(), refresh,
                    season:games_season(), query:games_query(), page:games_page(),
                    onpage:move |next|games_page.set(next),
                } },
                1..=3 => rsx!{ Statistics {
                    key: "stats-{view()}-{league()}-{stats_team}-{stats_season}-{qualified_only}-{locale:?}",
                    kind: view(), catalog: catalog.clone(), league: league(), team: stats_team,
                    season: stats_season, qualified_only, refresh
                } },
                4 => rsx!{ Teams {key:"team-{team()}",team:team(),refresh} },
                _ => rsx!{ super::management::Management {} },
            }
        }
    }
}

#[component]
fn Statistics(
    kind: u8,
    catalog: Catalog,
    league: u16,
    team: u16,
    season: u16,
    qualified_only: bool,
    refresh: u32,
) -> Element {
    let data = use_resource(use_reactive(
        (&kind, &league, &team, &season, &qualified_only, &refresh, &catalog),
        |(kind, league, team, season, qualified_only, _refresh, catalog)| async move {
            blocking(move || {
                if kind == 1 {
                    viewing::season_standings(season, league, &catalog)
                } else {
                    viewing::statistics_with_filter(kind, league, team, &catalog, qualified_only, season)
                }
            })
            .await
        },
    ));
    rsx! { match data.read().as_ref() {
        None=>rsx!{p {class:"loading",{tr("成績を読み込み中…（初回のみ集計します）")}}},
        Some(Err(e))=>rsx!{ErrorPanel {message:e.clone()}},
        Some(Ok(table))=>rsx!{
            DataTable {
                data: table.clone(),
                searchable: kind!=1,
                paginated: kind!=1,
                initial_sort: match kind {1=>None,2=>Some((5,true)),_=>Some((3,true))},
                empty_message: (qualified_only && table.rows.is_empty()).then_some("規定に達した選手はいません。"),
            }
        },
    } }
}
