// Copyright 2026 Ryan Duguid
// SPDX-License-Identifier: FSL-1.1-ALv2

use super::*;
use crate::core::embedded;

fn day(month: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, month, day).unwrap()
}

fn file(name: &str, text: &str) -> SourceFile {
    SourceFile {
        name: name.to_owned(),
        text: text.to_owned(),
    }
}

fn item_text(rank: u32, recheck: &str) -> String {
    format!(
        "rank = {rank}\ntitle = \"T\"\nwhy = \"W\"\nsteps = [\"S\"]\napplies = \"Everyone\"\n\
         effort = \"minutes\"\n{recheck}\n[[links]]\nname = \"N\"\nurl = \"https://example.gov.au/\"\n\
         checked = 2026-11-17\n"
    )
}

fn items() -> Vec<Item> {
    parse(&[
        file("b.toml", &item_text(2, "")),
        file("a.toml", &item_text(1, "recheck_months = 6")),
    ])
    .unwrap()
}

fn mark(item: &str, on: NaiveDate, mark: MarkKind) -> Mark {
    Mark {
        item: item.to_owned(),
        on,
        mark,
    }
}

#[test]
fn the_shipped_checklist_parses() {
    let items = parse(&embedded::checklist()).unwrap();
    assert!(!items.is_empty());
    assert!(items.windows(2).all(|w| w[0].rank < w[1].rank));
}

#[test]
fn items_sort_by_rank_and_take_their_id_from_the_file_name() {
    let ids: Vec<&str> = items().iter().map(|i| i.id.as_str()).collect();
    assert_eq!(ids, ["a", "b"]);
}

#[test]
fn every_problem_is_reported_before_any_item_is_returned() {
    let bad = item_text(1, "")
        .replace("https://", "http://")
        .replace("steps = [\"S\"]", "steps = []");
    let err = parse(&[
        file("Bad.toml", &bad),
        file("c.toml", &item_text(1, "recheck_months = 0")),
        file("d.toml", "rank = 3\nunknown = 1\n"),
        file("e.txt", ""),
    ])
    .unwrap_err();
    for expected in [
        "Bad.toml: the file name must be lowercase",
        "Bad.toml: title, why, applies and each step must have text",
        "Bad.toml: each link needs a name and an https:// URL",
        "c.toml: its rank is used by another item",
        "c.toml: recheck_months must be at least 1",
        "d.toml: unknown field `unknown`",
        "e.txt: not a .toml file",
    ] {
        assert!(err.contains(expected), "missing {expected:?} in:\n{err}");
    }
    let no_links = item_text(1, "");
    let no_links = &no_links[..no_links.find("[[links]]").unwrap()];
    let err = parse(&[file("f.toml", no_links)]).unwrap_err();
    assert!(err.contains("f.toml: it needs at least one link"), "{err}");
}

#[test]
fn the_release_gate_lists_links_checked_before_the_month() {
    let items = items();
    assert_eq!(unchecked_since(&items, "2026-11"), Ok(vec![]));
    assert_eq!(
        unchecked_since(&items, "2026-12"),
        Ok(vec![
            "a: https://example.gov.au/ (checked 2026-11-17)".to_owned(),
            "b: https://example.gov.au/ (checked 2026-11-17)".to_owned()
        ])
    );
    assert_eq!(
        unchecked_since(&items, "December"),
        Err("December is not a month such as 2026-12".to_owned())
    );
}

#[test]
fn a_done_item_is_checked_again_from_its_interval_onwards() {
    let items = items();
    let marks = [mark("a", day(3, 31), MarkKind::Done)];
    // 6 months after 31 March is 30 September.
    assert_eq!(
        progress(&items[0], &marks, day(9, 29)),
        Progress::Done(day(3, 31))
    );
    assert_eq!(
        progress(&items[0], &marks, day(9, 30)),
        Progress::CheckAgain(day(3, 31))
    );
    let once = [mark("b", day(1, 1), MarkKind::Done)];
    assert_eq!(
        progress(&items[1], &once, day(12, 31)),
        Progress::Done(day(1, 1))
    );
    let skipped = [mark("a", day(1, 1), MarkKind::NotForMe)];
    assert_eq!(
        progress(&items[0], &skipped, day(12, 31)),
        Progress::NotForMe
    );
    assert_eq!(progress(&items[1], &skipped, day(12, 31)), Progress::ToDo);
}

#[test]
fn next_prefers_an_item_to_check_again_then_the_first_to_do() {
    let items = items();
    assert_eq!(next(&items, &[], day(1, 1)).unwrap().id, "a");
    let a_done = [mark("a", day(1, 1), MarkKind::Done)];
    assert_eq!(next(&items, &a_done, day(2, 1)).unwrap().id, "b");
    let both = [a_done[0].clone(), mark("b", day(1, 1), MarkKind::Done)];
    assert!(next(&items, &both, day(2, 1)).is_none());
    assert_eq!(next(&items, &both, day(7, 1)).unwrap().id, "a");
    let gone = [mark("retired", day(1, 1), MarkKind::Done)];
    assert_eq!(next(&items, &gone, day(2, 1)).unwrap().id, "a");
}

#[test]
fn a_mark_replaces_the_earlier_one_and_clearing_removes_it() {
    let mut marks = vec![mark("retired", day(1, 1), MarkKind::Done)];
    assert!(set_mark(&mut marks, "a", Some((MarkKind::Done, day(1, 2)))));
    assert!(!set_mark(
        &mut marks,
        "a",
        Some((MarkKind::Done, day(1, 2)))
    ));
    assert!(set_mark(
        &mut marks,
        "a",
        Some((MarkKind::NotForMe, day(1, 3)))
    ));
    assert_eq!(
        marks,
        [
            mark("retired", day(1, 1), MarkKind::Done),
            mark("a", day(1, 3), MarkKind::NotForMe)
        ]
    );
    assert!(set_mark(&mut marks, "a", None));
    assert!(!set_mark(&mut marks, "a", None));
    assert_eq!(marks, [mark("retired", day(1, 1), MarkKind::Done)]);
}

#[test]
fn words_name_each_progress_and_effort() {
    assert_eq!(Progress::ToDo.words(), "to do");
    assert_eq!(Progress::Done(day(1, 2)).words(), "done 2026-01-02");
    assert_eq!(
        Progress::CheckAgain(day(1, 2)).words(),
        "check again (done 2026-01-02)"
    );
    assert_eq!(Progress::NotForMe.words(), "not for me");
    let words: Vec<&str> = [Effort::Minutes, Effort::AnHour, Effort::Longer]
        .iter()
        .map(|e| e.words())
        .collect();
    assert_eq!(words, ["minutes", "about an hour", "longer"]);
}
