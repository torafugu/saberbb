use super::locale::tr;
use dioxus::prelude::*;
use crate::application::viewing::TableData;

pub(super) fn compare_cells(a: &str, b: &str) -> std::cmp::Ordering {
    match (a.parse::<f64>(), b.parse::<f64>()) {
        (Ok(a), Ok(b)) => a.total_cmp(&b),
        _ => a.to_lowercase().cmp(&b.to_lowercase()),
    }
}

#[component]
pub(super) fn DataTable(data: TableData, #[props(default = true)] interactive: bool, #[props(default)] initial_sort: Option<(usize,bool)>) -> Element {
    let mut search = use_signal(String::new);
    let mut sort = use_signal(|| initial_sort);
    let mut page = use_signal(|| 0usize);
    let query = search().to_lowercase();
    let mut rows: Vec<_> = data.rows.iter().filter(|r| query.is_empty() || r.iter().any(|c| c.to_lowercase().contains(&query))).cloned().collect();
    if let Some((column, descending)) = sort() {
        rows.sort_by(|a,b| {
            let order = compare_cells(a.get(column).map(String::as_str).unwrap_or(""),b.get(column).map(String::as_str).unwrap_or(""));
            if descending { order.reverse() } else { order }
        });
    }
    let pages = rows.len().div_ceil(100).max(1);
    let current = page().min(pages - 1);
    let visible = rows.iter().skip(if interactive {current*100} else {0}).take(if interactive {100} else {usize::MAX});
    rsx! {
        section { class: "panel data-panel",
            div { class: "section-heading",
                h3 { {tr(&data.title)} }
                if interactive {
                    input { id:format!("table-search"), r#type: "search", aria_label: tr("表を検索"), placeholder: tr("選手・チームを検索…"), value: search,
                        oninput: move |e| { search.set(e.value()); page.set(0); } }
                }
            }
            if rows.is_empty() { p { class: "empty", {tr("表示できるデータがありません。管理TUIで作成・進行した後、再読み込みしてください。")} } }
            div { class: "table-scroll",
                table {
                    thead { tr {
                        for (i, column) in data.columns.iter().enumerate() {
                            th { aria_sort: if sort().is_some_and(|(c,_)|c==i) { if sort().unwrap().1 { "descending" } else { "ascending" } } else { "none" },
                                if interactive {
                                    button { id:format!("sort-{i}"), onclick: move |_| { let descending = sort().is_some_and(|(c,d)|c==i && !d); sort.set(Some((i,descending)));page.set(0); }, {tr(column)} }
                                } else { {tr(column)} }
                            }
                        }
                    } }
                    tbody {
                        for (index, row) in visible.enumerate() {
                            tr { key: "{index}", for cell in row { td { "{cell}" } } }
                        }
                    }
                }
            }
            if interactive {
                div { class: "pagination",
                    span { {format!("{} {}",rows.len(),tr("件"))} }
                    button { id:format!("table-page-prev"), disabled: current==0, onclick: move |_| page.set(current.saturating_sub(1)), {tr("前へ")} }
                    span { "{current+1} / {pages}" }
                    button { id:format!("table-page-next"), disabled: current+1>=pages, onclick: move |_| page.set((current+1).min(pages-1)), {tr("次へ")} }
                }
            }
        }
    }
}

fn error_help(message: &str) -> &'static str {
    if message.contains("database is locked") || message.contains("database is busy") {
        "別の処理がDBを使用中です。集計・試合保存の完了を待ち、DB編集ツールのトランザクションを終了して再読み込みしてください。"
    } else {
        "DB設定と初期化を確認し、再読み込みしてください。"
    }
}

#[component]
pub(super) fn ErrorPanel(message: String) -> Element {
    rsx! { div { class: "error", role: "alert", h3 { {tr("読み込みできませんでした")} } p { "{message}" } p { {tr(error_help(&message))} } } }
}

#[cfg(all(test, feature="gui-tests"))]
mod tests {
    use super::*;
    #[test]
    fn numeric_columns_sort_numerically() {
        assert!(compare_cells("12", "3").is_gt());
        assert!(compare_cells(".250", ".310").is_lt());
    }
    #[test]
    fn ssr_table_escapes_names_and_preserves_table_structure() {
        fn fixture() -> Element {
            rsx! { DataTable { data: TableData::new("打撃成績", &["選手","HR"],vec![vec!["<script>alert(1)</script>".into(),"12".into()]]), interactive:false } }
        }
        let mut dom = VirtualDom::new(fixture); dom.rebuild_in_place();
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains("<table")); assert!(html.contains("&#60;script&#62;")); assert!(!html.contains("<script>alert"));
    }
}

#[cfg(test)]
mod lock_error_tests {
    #[test]
    fn locked_database_does_not_suggest_initializing_the_database() {
        let help = super::error_help("Internal error: database is locked: Error code 5");
        assert!(help.contains("完了を待ち"));
        assert!(!help.contains("初期化"));
    }
}
