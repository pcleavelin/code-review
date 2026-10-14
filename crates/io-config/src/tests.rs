use std::fs;
use std::path::{Path, PathBuf};
use std::process;

use super::*;

const SAMPLE: &str = "review layout 1
down 820
  right 220
    panel Paths* Symbols Files
    right 740
      panel Path Diff Graph* Source Search
      panel References*
  panel Console*
column title 48
column author 12
";

fn scratch(name: &str) -> PathBuf {
    let folder = env::temp_dir().join(format!("io-config-{name}-{}", process::id()));
    drop(fs::remove_dir_all(&folder));
    folder
}

#[test]
fn a_layout_reads_and_prints_back_byte_for_byte() {
    let wire = wire::parse(SAMPLE).unwrap();
    let layout = convert::layout(&wire).unwrap();
    assert_eq!(wire::print(&convert::wire(&layout)), SAMPLE);
}

#[test]
fn a_save_creates_the_folder_and_a_load_returns_the_same_tree() {
    let folder = scratch("round");
    let store = LayoutStore::at(folder.join("code-review").join("layout"));
    let layout = convert::layout(&wire::parse(SAMPLE).unwrap()).unwrap();
    store.save(&layout).unwrap();
    assert_eq!(fs::read_to_string(store.path()).unwrap(), SAMPLE);
    assert_eq!(store.load(), Some(layout));
    fs::remove_dir_all(folder).unwrap();
}

#[test]
fn a_layout_without_column_widths_reads_with_none() {
    let layout = convert::layout(&wire::parse("review layout 1\npanel A* B\n").unwrap()).unwrap();
    assert!(layout.columns.is_empty());
    assert!(matches!(layout.tree, domain::LayoutTree::Panel(_)));
}

#[test]
fn anything_malformed_reads_as_no_layout() {
    for text in [
        "",
        "review layout 2\npanel Paths\n",
        "review layout 1\nsideways 500\n  panel A\n  panel B\n",
        "review layout 1\ndown 1001\n  panel A\n  panel B\n",
        "review layout 1\ndown 500\n  panel A\n",
        "review layout 1\npanel A\npanel B\n",
        "review layout 1\n panel A\n",
        "review layout 1\npanel A-B\n",
        "review layout 1\npanel A\ncolumn title\n",
        "review layout 1\npanel A\ncolumn title 2\n",
        "review layout 1\npanel A\ncolumn ti_tle 20\n",
        "review layout 1\npanel A\n  column title 20\n",
    ] {
        let parsed = wire::parse(text).and_then(|wire| convert::layout(&wire));
        assert_eq!(parsed, None, "{text:?}");
    }
    let missing = LayoutStore::at(scratch("missing").join("layout"));
    assert_eq!(missing.load(), None);
}

const SETTINGS_SAMPLE: &str = "review settings 1
theme light
font JetBrains Mono
size 17
";

#[test]
fn settings_read_and_print_back_byte_for_byte() {
    let wire = wire::parse_settings(SETTINGS_SAMPLE).unwrap();
    let settings = convert::settings(&wire);
    assert_eq!(settings.theme(), domain::Theme::Light);
    assert_eq!(
        settings.font().map(domain::FontFamily::as_str),
        Some("JetBrains Mono")
    );
    assert_eq!(settings.size().get(), 17);
    assert_eq!(
        wire::print_settings(&convert::wire_settings(&settings)),
        SETTINGS_SAMPLE
    );
}

#[test]
fn the_bundled_font_is_written_as_no_font_line() {
    let settings = domain::Settings::default();
    assert_eq!(
        wire::print_settings(&convert::wire_settings(&settings)),
        "review settings 1\ntheme dark\nsize 14\n"
    );
}

#[test]
fn a_bad_line_keeps_that_setting_at_its_default_and_the_rest_as_written() {
    let text = "review settings 1\ntheme purple\nsize 99\ncolour red\n";
    let settings = convert::settings(&wire::parse_settings(text).unwrap());
    assert_eq!(settings, domain::Settings::default());
}

#[test]
fn another_header_reads_as_no_settings() {
    assert_eq!(
        wire::parse_settings("review settings 2\ntheme light\n"),
        None
    );
    assert_eq!(wire::parse_settings(""), None);
    let missing = SettingsStore::at(scratch("settings-missing").join("settings"));
    assert_eq!(missing.load(), None);
}

#[test]
fn a_settings_save_creates_the_folder_and_a_load_returns_the_same_settings() {
    let folder = scratch("settings-round");
    let store = SettingsStore::at(folder.join("code-review").join("settings"));
    let settings = convert::settings(&wire::parse_settings(SETTINGS_SAMPLE).unwrap());
    store.save(&settings).unwrap();
    assert_eq!(fs::read_to_string(store.path()).unwrap(), SETTINGS_SAMPLE);
    assert_eq!(store.load(), Some(settings));
    fs::remove_dir_all(folder).unwrap();
}

#[test]
fn pins_round_trip() {
    let folder = env::temp_dir().join(format!("io-config-pins-{}", process::id()));
    drop(fs::remove_dir_all(&folder));
    let store = PinsStore::at(folder.join("pins"), None);
    assert!(store.load().is_empty());
    let pins = vec![
        domain::Root::new(Path::new("/work/one")),
        domain::Root::new(Path::new("C:/work/two words")),
    ];
    store.save(&pins).unwrap();
    assert_eq!(store.load(), pins);
    fs::remove_dir_all(folder).unwrap();
}

#[test]
fn the_forges_file_gives_each_hosts_client_id_and_skips_lines_it_cannot_read() {
    let folder = scratch("forges");
    fs::create_dir_all(&folder).unwrap();
    let path = folder.join("forges");
    let store = ForgesStore::at(path.clone());
    assert!(store.load().is_empty());
    io_store::write(
        &path,
        "review forges 1\nclient gitlab.example.org 8e1f2a\nclient github.com\nclient a b c\ntheme dark\nclient GitHub.com Iv23override\n",
    )
    .unwrap();
    let apps: Vec<(String, String)> = store
        .load()
        .iter()
        .map(|app| (app.host.to_string(), app.client.to_string()))
        .collect();
    assert_eq!(
        apps,
        [
            ("gitlab.example.org".to_owned(), "8e1f2a".to_owned()),
            ("github.com".to_owned(), "Iv23override".to_owned()),
        ]
    );
    io_store::write(&path, "review settings 1\nclient github.com x\n").unwrap();
    assert!(store.load().is_empty());
    fs::remove_dir_all(folder).unwrap();
}

#[test]
fn the_forges_file_names_the_jira_sites_and_skips_lines_it_cannot_read() {
    let folder = scratch("jira");
    fs::create_dir_all(&folder).unwrap();
    let path = folder.join("forges");
    let store = ForgesStore::at(path.clone());
    io_store::write(
        &path,
        "review forges 1\njira acme.atlassian.net\nclient github.com x\njira\njira a b\njira jira.example.org\n",
    )
    .unwrap();
    let sites: Vec<String> = store.jira_sites().iter().map(ToString::to_string).collect();
    assert_eq!(sites, ["acme.atlassian.net", "jira.example.org"]);
    assert_eq!(store.load().len(), 1);
    fs::remove_dir_all(folder).unwrap();
}

#[test]
fn a_jira_site_is_added_to_the_forges_file_once_and_its_other_lines_stay() {
    assert_eq!(
        wire::with_jira(None, "acme.atlassian.net").as_deref(),
        Some("review forges 1\njira acme.atlassian.net\n")
    );
    let kept = "review forges 1\nclient gitlab.test cid";
    assert_eq!(
        wire::with_jira(Some(kept), "acme.atlassian.net").as_deref(),
        Some("review forges 1\nclient gitlab.test cid\njira acme.atlassian.net\n")
    );
    let known = "review forges 1\njira ACME.atlassian.net\n";
    assert_eq!(
        wire::with_jira(Some(known), "acme.atlassian.net").as_deref(),
        Some(known)
    );
    assert_eq!(
        wire::with_jira(Some("something else\n"), "acme.atlassian.net"),
        None
    );
}
