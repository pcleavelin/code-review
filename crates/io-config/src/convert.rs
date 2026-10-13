use std::path::Path;

use domain::{
    BaseFontSize, ClientId, ColumnCells, ColumnKey, ColumnWidth, FontFamily, Host, JiraSite,
    Layout, LayoutPanel, LayoutSplit, LayoutTree, OAuthApp, Root, Settings, Share, SplitDirection,
    Theme, ViewKey,
};

use crate::wire::{
    WireColumn, WireDirection, WireForges, WireLayout, WirePins, WireSetting, WireSettings,
    WireTheme, WireTree, WireView,
};

pub(crate) fn layout(wire: &WireLayout) -> Option<Layout> {
    Some(Layout {
        tree: tree(&wire.tree)?,
        columns: wire
            .columns
            .iter()
            .map(|column| {
                Some(ColumnWidth {
                    column: ColumnKey::new(&column.name)?,
                    cells: ColumnCells::new(column.cells)?,
                })
            })
            .collect::<Option<Vec<ColumnWidth>>>()?,
    })
}

pub(crate) fn wire(layout: &Layout) -> WireLayout {
    WireLayout {
        tree: wire_tree(&layout.tree),
        columns: layout
            .columns
            .iter()
            .map(|width| WireColumn {
                name: width.column.as_str().to_owned(),
                cells: width.cells.get(),
            })
            .collect(),
    }
}

fn tree(wire: &WireTree) -> Option<LayoutTree> {
    match wire {
        WireTree::Panel { views } => {
            let keys = views
                .iter()
                .map(|view| ViewKey::new(&view.name))
                .collect::<Option<Vec<ViewKey>>>()?;
            let shown = views
                .iter()
                .find(|view| view.shown)
                .and_then(|view| ViewKey::new(&view.name));
            Some(LayoutTree::Panel(LayoutPanel::new(keys, shown)))
        }
        WireTree::Split {
            direction: split,
            share,
            first,
            second,
        } => Some(LayoutTree::Split(LayoutSplit::new(
            direction(*split),
            Share::permille(*share)?,
            tree(first)?,
            tree(second)?,
        ))),
    }
}

fn wire_tree(layout: &LayoutTree) -> WireTree {
    match layout {
        LayoutTree::Panel(panel) => WireTree::Panel {
            views: panel
                .views()
                .iter()
                .map(|key| WireView {
                    name: key.as_str().to_owned(),
                    shown: panel.shown() == Some(key),
                })
                .collect(),
        },
        LayoutTree::Split(split) => WireTree::Split {
            direction: wire_direction(split.direction()),
            share: split.share().get(),
            first: Box::new(wire_tree(split.first())),
            second: Box::new(wire_tree(split.second())),
        },
    }
}

const fn direction(wire: WireDirection) -> SplitDirection {
    match wire {
        WireDirection::Right => SplitDirection::Right,
        WireDirection::Down => SplitDirection::Down,
    }
}

const fn wire_direction(direction: SplitDirection) -> WireDirection {
    match direction {
        SplitDirection::Right => WireDirection::Right,
        SplitDirection::Down => WireDirection::Down,
    }
}

pub(crate) fn settings(wire: &WireSettings) -> Settings {
    wire.entries
        .iter()
        .fold(Settings::default(), |settings, entry| match entry {
            WireSetting::Theme(WireTheme::Dark) => settings.with_theme(Theme::Dark),
            WireSetting::Theme(WireTheme::Light) => settings.with_theme(Theme::Light),
            WireSetting::Font(family) => match FontFamily::new(family) {
                Some(family) => settings.with_font(Some(family)),
                None => settings,
            },
            WireSetting::Size(size) => match BaseFontSize::new(*size) {
                Some(size) => settings.with_size(size),
                None => settings,
            },
        })
}

pub(crate) fn wire_settings(settings: &Settings) -> WireSettings {
    let theme = match settings.theme() {
        Theme::Dark => WireTheme::Dark,
        Theme::Light => WireTheme::Light,
    };
    let mut entries = vec![WireSetting::Theme(theme)];
    if let Some(family) = settings.font() {
        entries.push(WireSetting::Font(family.as_str().to_owned()));
    }
    entries.push(WireSetting::Size(settings.size().get()));
    WireSettings { entries }
}

pub(crate) fn pins(wire: &WirePins) -> Vec<Root> {
    wire.paths
        .iter()
        .map(|path| Root::new(Path::new(path)))
        .collect()
}

pub(crate) fn wire_pins(pins: &[Root]) -> WirePins {
    WirePins {
        paths: pins
            .iter()
            .map(|root| root.as_path().to_string_lossy().into_owned())
            .collect(),
    }
}

pub(crate) fn jira_sites(wire: &WireForges) -> Vec<JiraSite> {
    wire.jira
        .iter()
        .filter_map(|host| Host::new(host).map(JiraSite::new))
        .collect()
}

pub(crate) fn apps(wire: &WireForges) -> Vec<OAuthApp> {
    wire.apps
        .iter()
        .filter_map(|app| {
            Some(OAuthApp {
                host: Host::new(&app.host)?,
                client: ClientId::new(&app.client)?,
            })
        })
        .collect()
}
