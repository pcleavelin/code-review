use domain::RelativePath;
use ui::{Count, Id, Label};

use crate::keys::{self, Chord};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Element(&'static str);

impl Element {
    const fn new(name: &'static str) -> Self {
        Self(name)
    }

    pub(crate) const fn as_str(self) -> &'static str {
        self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Words(&'static str);

impl Words {
    const fn new(text: &'static str) -> Self {
        Self(text)
    }

    pub(crate) fn spelled(self) -> Label {
        Label::new(self.0.replace(PRIMARY_MARK.0, keys::PRIMARY_NAME.as_str()))
    }
}

const PRIMARY_MARK: Words = Words::new("{primary}");

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Tip {
    pub(crate) words: Words,
    pub(crate) chord: Option<Chord>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Target {
    id: Id,
    tip: Tip,
}

impl Target {
    pub(crate) const fn id(self) -> Id {
        self.id
    }

    pub(crate) const fn tip(self) -> Tip {
        self.tip
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Control {
    element: Element,
    tip: Tip,
}

impl Control {
    const fn new(element: Element, words: Words) -> Self {
        Self {
            element,
            tip: Tip { words, chord: None },
        }
    }

    const fn keyed(mut self, chord: Chord) -> Self {
        self.tip.chord = Some(chord);
        self
    }

    pub(crate) fn id(self) -> Id {
        Id::from_name(self.element.as_str())
    }

    const fn aim(self, id: Id) -> Target {
        Target { id, tip: self.tip }
    }

    pub(crate) fn target(self) -> Target {
        self.aim(self.id())
    }

    pub(crate) fn nth(self, number: Count) -> Target {
        self.aim(self.id().nth(number.get()))
    }

    pub(crate) fn with(self, suffix: &Label) -> Target {
        self.aim(self.id().with(suffix.as_str()))
    }

    pub(crate) const fn element(self) -> Element {
        self.element
    }
}

pub(crate) const SEARCH_FIELD: Control = Control::new(
    Element::new("field@search"),
    Words::new("search every file of the tree for a regex; enter runs it"),
);
pub(crate) const FILTER_FIELD: Control = Control::new(
    Element::new("field@symbols"),
    Words::new("filter the symbols by name or file"),
);
pub(crate) const COMMAND_FIELD: Control = Control::new(
    Element::new("cmd"),
    Words::new("run a CLI command; it acts on the same reviews"),
);
pub(crate) const BACK: Control = Control::new(
    Element::new("back"),
    Words::new("go back to the previous place"),
)
.keyed(keys::GO_BACK);
pub(crate) const FORWARD: Control =
    Control::new(Element::new("forward"), Words::new("go forward again")).keyed(keys::GO_FORWARD);
pub(crate) const TAB: Control = Control::new(
    Element::new("tab"),
    Words::new("show this view; drag the tab onto another panel to move it"),
);
pub(crate) const DIVIDER: Control = Control::new(
    Element::new("divider"),
    Words::new("drag to resize the panels on either side"),
);
pub(crate) const SPLIT_RIGHT: Control = Control::new(
    Element::new("split-right"),
    Words::new("split this panel, the new half to the right"),
);
pub(crate) const SPLIT_DOWN: Control = Control::new(
    Element::new("split-down"),
    Words::new("split this panel, the new half below"),
);
pub(crate) const CLOSE_PANEL: Control = Control::new(
    Element::new("close-panel"),
    Words::new("close this panel; its neighbour takes its place"),
);
pub(crate) const CLOSE_TAB: Control =
    Control::new(Element::new("close-tab"), Words::new("close this view"));
pub(crate) const PICK: Control = Control::new(
    Element::new("pick"),
    Words::new("choose a view for this panel"),
);
pub(crate) const VIEW_ROW: Control =
    Control::new(Element::new("view"), Words::new("show this view here"));
pub(crate) const VIEW_FIELD: Control = Control::new(
    Element::new("field@views"),
    Words::new("type to find a view"),
);
pub(crate) const SYMBOL_ROW: Control = Control::new(
    Element::new("sym"),
    Words::new("show this symbol in the Source view"),
);
pub(crate) const SYMBOL_FILE_ROW: Control =
    Control::new(Element::new("symfile"), Words::new("open this file"));
pub(crate) const DIRECTORY_ROW: Control = Control::new(
    Element::new("dir"),
    Words::new("open or close this directory"),
);
pub(crate) const FILE_ROW: Control = Control::new(
    Element::new("file"),
    Words::new("open this file; a changed one opens at its first change"),
);
pub(crate) const CALLER_ROW: Control =
    Control::new(Element::new("xto"), Words::new("go to this caller"));
pub(crate) const CALLEE_ROW: Control =
    Control::new(Element::new("xfrom"), Words::new("go to this callee"));
pub(crate) const REFERENCE_ROW: Control =
    Control::new(Element::new("xref"), Words::new("go to this reference"));
pub(crate) const REFERENCE_FILE_ROW: Control =
    Control::new(Element::new("xreffile"), Words::new("open this file"));
pub(crate) const PEEK_GO: Control = Control::new(
    Element::new("peek-go"),
    Words::new("go to the definition shown here"),
);
pub(crate) const PEEK_CLOSE: Control = Control::new(
    Element::new("peek-x"),
    Words::new("close the peeked definition"),
);
pub(crate) const LINES: Control = Control::new(
    Element::new("lines"),
    Words::new(
        "click a line to select it, drag to select several; double-click or {primary}+click a name to go to its definition, alt+click to peek it",
    ),
);
pub(crate) const HIT_ROW: Control = Control::new(Element::new("hit"), Words::new("go to this hit"));
pub(crate) const HIT_FILE_ROW: Control =
    Control::new(Element::new("hitfile"), Words::new("open this file"));
pub(crate) const INBOX_FIELD: Control = Control::new(
    Element::new("field@inbox"),
    Words::new("filter by any words of the title, repository, author, branch or #number"),
);
pub(crate) const INBOX_STATE: Control = Control::new(
    Element::new("inbox-state"),
    Words::new("show only the pull requests in this state"),
);
pub(crate) const INBOX_REQUESTED: Control = Control::new(
    Element::new("inbox-requested"),
    Words::new(
        "show only the pull requests whose review is requested from you, teams included; needs a login to their forge",
    ),
);
pub(crate) const PULL_ROW: Control = Control::new(
    Element::new("pull"),
    Words::new("open this pull request: fetch it, check out its head and index it"),
);
pub(crate) const PULLS_REFRESH: Control = Control::new(
    Element::new("pulls-refresh"),
    Words::new("ask every pinned repository's forge again for its open pull requests"),
);
pub(crate) const PIN_FIELD: Control = Control::new(
    Element::new("field@pin"),
    Words::new("a local clone's path, or a repository URL to clone; enter pins it"),
);
pub(crate) const PIN_ADD: Control = Control::new(
    Element::new("pin"),
    Words::new("pin the repository typed beside it"),
);
pub(crate) const UNPIN: Control = Control::new(
    Element::new("unpin"),
    Words::new("unpin this repository; its reviews stay on disk"),
);
pub(crate) const LOGIN: Control = Control::new(
    Element::new("login"),
    Words::new("log in to this forge; the token goes to the system keychain"),
);
pub(crate) const LOGOUT: Control = Control::new(
    Element::new("logout"),
    Words::new("log out of this forge: remove its token from the system keychain"),
);
pub(crate) const LOGIN_COPY: Control = Control::new(
    Element::new("login-copy"),
    Words::new("copy the code to enter on the forge's page"),
);
pub(crate) const LOGIN_OPEN: Control = Control::new(
    Element::new("login-open"),
    Words::new("open the forge's login page in the browser again"),
);
pub(crate) const LOGIN_CANCEL: Control = Control::new(
    Element::new("login-cancel"),
    Words::new("stop this login; nothing is kept"),
);
pub(crate) const LOGIN_SAVE: Control = Control::new(
    Element::new("login-save"),
    Words::new("keep the pasted token in the system keychain"),
);
pub(crate) const TOKEN_FIELD: Control = Control::new(
    Element::new("field@token"),
    Words::new("paste the access token made on the forge's page; enter keeps it"),
);
pub(crate) const JIRA_SITE_FIELD: Control = Control::new(
    Element::new("field@jira-site"),
    Words::new("the Jira site's address, or any link to it (yourcompany.atlassian.net)"),
);
pub(crate) const JIRA_EMAIL_FIELD: Control = Control::new(
    Element::new("field@jira-email"),
    Words::new(
        "the email you sign in to Jira Cloud with; empty for a Data Center personal access token",
    ),
);
pub(crate) const JIRA_LOGIN: Control = Control::new(
    Element::new("jira-login"),
    Words::new(
        "log in to Jira: its site, your email and an API token, checked with Jira and kept in the system keychain",
    ),
);
pub(crate) const JIRA_TOKENS: Control = Control::new(
    Element::new("jira-tokens"),
    Words::new("open Atlassian's page that makes API tokens"),
);
pub(crate) const VIEWER_OUT: Control = Control::new(
    Element::new("viewer-out"),
    Words::new("zoom out (-); pinch or {primary}+wheel zoom where the pointer is"),
);
pub(crate) const VIEWER_FIT: Control = Control::new(
    Element::new("viewer-fit"),
    Words::new("fit the picture in the window (0)"),
);
pub(crate) const VIEWER_ACTUAL: Control = Control::new(
    Element::new("viewer-actual"),
    Words::new("show the picture at its own size, one pixel to a pixel (1)"),
);
pub(crate) const VIEWER_IN: Control = Control::new(
    Element::new("viewer-in"),
    Words::new("zoom in (+); drag or scroll to move around"),
);
pub(crate) const VIEWER_BROWSER: Control = Control::new(
    Element::new("viewer-browser"),
    Words::new("open the picture in the browser"),
);
pub(crate) const VIEWER_CLOSE: Control = Control::new(
    Element::new("viewer-close"),
    Words::new("close the picture (escape)"),
);
pub(crate) const CHANGE_ROW: Control = Control::new(
    Element::new("change"),
    Words::new("show this changed file at its first change"),
);
pub(crate) const SEEN: Control = Control::new(
    Element::new("seen"),
    Words::new(
        "mark the file seen, or unseen again; it turns unseen by itself when its diff changes",
    ),
)
.keyed(keys::TOGGLE_SEEN);
pub(crate) const NEXT_UNSEEN: Control = Control::new(
    Element::new("next-unseen"),
    Words::new("show the next changed file not seen yet"),
)
.keyed(keys::NEXT_UNSEEN);
pub(crate) const NEXT_HUNK: Control = Control::new(
    Element::new("next-hunk"),
    Words::new("go to the next change in this file, then the next file"),
)
.keyed(keys::NEXT_HUNK);
pub(crate) const PREVIOUS_HUNK: Control = Control::new(
    Element::new("previous-hunk"),
    Words::new("go to the previous change in this file"),
)
.keyed(keys::PREVIOUS_HUNK);
pub(crate) const WHOLE_FILE: Control = Control::new(
    Element::new("whole-file"),
    Words::new("show the whole file, or only the changes and the lines around them"),
);
pub(crate) const LINK_OPEN: Control = Control::new(
    Element::new("link-open"),
    Words::new("open it on the forge, in the browser"),
);
pub(crate) const LINK_COPY: Control = Control::new(
    Element::new("link-copy"),
    Words::new("copy its link on the forge"),
);
pub(crate) const AUTHOR: Control = Control::new(
    Element::new("author"),
    Words::new("open this person's page on the forge"),
);
pub(crate) const TABLE: Control = Control::new(
    Element::new("table"),
    Words::new("click a line to select it, shift-click or drag for more; type to comment on it"),
);
pub(crate) const LAYOUT: Control = Control::new(
    Element::new("layout"),
    Words::new("show the diff side by side, or as one column again"),
);
pub(crate) const ALL_CHANGES: Control = Control::new(
    Element::new("all-changes"),
    Words::new("back to the whole pull request's changes"),
);
pub(crate) const COLLAPSE: Control = Control::new(
    Element::new("collapse"),
    Words::new("fold this file's diff away under its name, or show it again"),
);
pub(crate) const COMMIT_ROW: Control = Control::new(
    Element::new("commit"),
    Words::new("show only this commit's changes"),
);
pub(crate) const MARKDOWN: Control = Control::new(
    Element::new("markdown"),
    Words::new("text written on the forge"),
);
pub(crate) const MD_LINK: Control = Control::new(
    Element::new("md-link"),
    Words::new("open this link in the browser"),
);
pub(crate) const MD_SOURCE: Control = Control::new(
    Element::new("md-source"),
    Words::new(
        "open this file of the pull request in the Source view, at its head, with the linked lines selected",
    ),
);
pub(crate) const MD_CODE: Control = Control::new(
    Element::new("md-code"),
    Words::new("code quoted in the text"),
);
pub(crate) const PICTURE: Control = Control::new(
    Element::new("picture"),
    Words::new("open the picture to zoom in and move around it"),
);
pub(crate) const FOLD: Control = Control::new(
    Element::new("fold"),
    Words::new("show all of these unchanged lines"),
);
pub(crate) const FOLD_UP: Control = Control::new(
    Element::new("fold-up"),
    Words::new("show 20 more unchanged lines above the change below"),
);
pub(crate) const FOLD_DOWN: Control = Control::new(
    Element::new("fold-down"),
    Words::new("show 20 more unchanged lines below the change above"),
);
pub(crate) const DRAFT_HERE: Control = Control::new(
    Element::new("draft-here"),
    Words::new("comment on the selected line"),
)
.keyed(keys::COMMENT);
pub(crate) const COPY_LINES: Control = Control::new(
    Element::new("copy-lines"),
    Words::new("copy the selected lines as the file has them"),
)
.keyed(keys::COPY_LINES);
pub(crate) const DRAFT_REMOVED: Control = Control::new(
    Element::new("draft-removed"),
    Words::new("comment on this removed line"),
);
pub(crate) const DRAFT_FIELD: Control = Control::new(
    Element::new("field@draft"),
    Words::new("the comment; {primary}+enter adds it, escape cancels"),
);
pub(crate) const DRAFT_ADD: Control = Control::new(
    Element::new("draft-add"),
    Words::new("keep this comment as a draft; it is sent with the review"),
);
pub(crate) const DRAFT_CANCEL: Control = Control::new(
    Element::new("draft-cancel"),
    Words::new("throw this comment away"),
);
pub(crate) const DRAFT_REMOVE: Control = Control::new(
    Element::new("draft-remove"),
    Words::new("remove this draft before it is sent"),
);
pub(crate) const COLUMN_FIELD: Control = Control::new(
    Element::new("column-filter"),
    Words::new("keep the pull requests whose cell in this column has these words"),
);
pub(crate) const COLUMN_HEADER: Control = Control::new(
    Element::new("column-header"),
    Words::new("sort by this column; again to reverse"),
);
pub(crate) const COLUMN_EDGE: Control = Control::new(
    Element::new("column-edge"),
    Words::new("drag to make this column wider or narrower"),
);
pub(crate) const CARD_LINK: Control = Control::new(
    Element::new("card-link"),
    Words::new("open the Jira card in the browser"),
);
pub(crate) const CARD_COLLAPSE: Control = Control::new(
    Element::new("card-collapse"),
    Words::new("show or hide the Jira card's description"),
);
pub(crate) const DRAFT_EDIT: Control = Control::new(
    Element::new("draft-edit"),
    Words::new("change the text of this draft before it is sent"),
);
pub(crate) const REPLY: Control = Control::new(
    Element::new("reply"),
    Words::new("answer this thread; the answer is a draft until the review is sent"),
);
pub(crate) const THREAD_ROW: Control = Control::new(
    Element::new("thread"),
    Words::new("show this thread where it is in the code"),
);
pub(crate) const TALK_REFRESH: Control = Control::new(
    Element::new("talk-refresh"),
    Words::new("read the description and the discussion again from the forge"),
);
pub(crate) const SUMMARY_FIELD: Control = Control::new(
    Element::new("field@summary"),
    Words::new("what the review says as a whole, or an answer to the conversation"),
);
pub(crate) const SUBMIT_COMMENT: Control = Control::new(
    Element::new("submit-comment"),
    Words::new("send the drafts and the summary as a review that only comments"),
);
pub(crate) const SUBMIT_APPROVE: Control = Control::new(
    Element::new("submit-approve"),
    Words::new("send the drafts and the summary and approve"),
);
pub(crate) const SUBMIT_CHANGES: Control = Control::new(
    Element::new("submit-changes"),
    Words::new("send the drafts and the summary and request changes"),
);
pub(crate) const SUBMIT_CONFIRM: Control = Control::new(
    Element::new("submit-confirm"),
    Words::new("send the review to the forge now"),
);
pub(crate) const SUBMIT_CANCEL: Control = Control::new(
    Element::new("submit-cancel"),
    Words::new("keep the review here unsent"),
);
pub(crate) const REVIEW_DRAFT: Control = Control::new(
    Element::new("review-draft"),
    Words::new("show this draft where it is in the code"),
);
pub(crate) const PALETTE_FIELD: Control = Control::new(
    Element::new("field@palette"),
    Words::new("type to find a pull request, changed file, symbol, file, view or action"),
)
.keyed(keys::PALETTE);
pub(crate) const PALETTE_ROW: Control =
    Control::new(Element::new("palette"), Words::new("go there, or run it"));
pub(crate) const SETTINGS_OPEN: Control = Control::new(
    Element::new("settings-open"),
    Words::new("the theme, the font and its size"),
)
.keyed(keys::SETTINGS);
pub(crate) const SETTINGS_CLOSE: Control = Control::new(
    Element::new("settings-close"),
    Words::new("close the settings"),
);
pub(crate) const SETTINGS_THEME: Control =
    Control::new(Element::new("settings-theme"), Words::new("use this theme"));
pub(crate) const SETTINGS_SMALLER: Control = Control::new(
    Element::new("settings-smaller"),
    Words::new("a smaller font"),
);
pub(crate) const SETTINGS_LARGER: Control =
    Control::new(Element::new("settings-larger"), Words::new("a larger font"));
pub(crate) const SETTINGS_FONT: Control =
    Control::new(Element::new("settings-font"), Words::new("use this font"));
pub(crate) const ELEMENT_TIP: Control = Control::new(
    Element::new("element-tip"),
    Words::new("what the button under the pointer does"),
);

#[cfg(test)]
pub(crate) const CONTROLS: &[Control] = &[
    SEARCH_FIELD,
    FILTER_FIELD,
    COMMAND_FIELD,
    BACK,
    FORWARD,
    TAB,
    DIVIDER,
    SPLIT_RIGHT,
    SPLIT_DOWN,
    CLOSE_PANEL,
    CLOSE_TAB,
    PICK,
    VIEW_ROW,
    VIEW_FIELD,
    SYMBOL_ROW,
    SYMBOL_FILE_ROW,
    DIRECTORY_ROW,
    FILE_ROW,
    CALLER_ROW,
    CALLEE_ROW,
    REFERENCE_ROW,
    REFERENCE_FILE_ROW,
    PEEK_GO,
    PEEK_CLOSE,
    LINES,
    HIT_ROW,
    HIT_FILE_ROW,
    INBOX_FIELD,
    INBOX_STATE,
    INBOX_REQUESTED,
    PULL_ROW,
    PULLS_REFRESH,
    PIN_FIELD,
    PIN_ADD,
    UNPIN,
    LOGIN,
    LOGOUT,
    LOGIN_COPY,
    LOGIN_OPEN,
    LOGIN_CANCEL,
    LOGIN_SAVE,
    TOKEN_FIELD,
    JIRA_SITE_FIELD,
    JIRA_EMAIL_FIELD,
    JIRA_LOGIN,
    JIRA_TOKENS,
    VIEWER_OUT,
    VIEWER_FIT,
    VIEWER_ACTUAL,
    VIEWER_IN,
    VIEWER_BROWSER,
    VIEWER_CLOSE,
    CHANGE_ROW,
    SEEN,
    NEXT_UNSEEN,
    NEXT_HUNK,
    PREVIOUS_HUNK,
    WHOLE_FILE,
    MARKDOWN,
    MD_LINK,
    MD_SOURCE,
    MD_CODE,
    PICTURE,
    TABLE,
    LAYOUT,
    ALL_CHANGES,
    COLLAPSE,
    COMMIT_ROW,
    LINK_OPEN,
    LINK_COPY,
    AUTHOR,
    FOLD,
    FOLD_UP,
    FOLD_DOWN,
    DRAFT_HERE,
    COPY_LINES,
    DRAFT_REMOVED,
    DRAFT_FIELD,
    DRAFT_ADD,
    DRAFT_CANCEL,
    DRAFT_REMOVE,
    DRAFT_EDIT,
    CARD_LINK,
    CARD_COLLAPSE,
    COLUMN_FIELD,
    COLUMN_HEADER,
    COLUMN_EDGE,
    REPLY,
    THREAD_ROW,
    TALK_REFRESH,
    SUMMARY_FIELD,
    SUBMIT_COMMENT,
    SUBMIT_APPROVE,
    SUBMIT_CHANGES,
    SUBMIT_CONFIRM,
    SUBMIT_CANCEL,
    REVIEW_DRAFT,
    PALETTE_FIELD,
    PALETTE_ROW,
    SETTINGS_OPEN,
    SETTINGS_CLOSE,
    SETTINGS_THEME,
    SETTINGS_SMALLER,
    SETTINGS_LARGER,
    SETTINGS_FONT,
    ELEMENT_TIP,
];

pub(crate) fn in_file(path: &RelativePath, at: Count) -> Label {
    Label::new(format!("{path}:{at}"))
}

pub(crate) fn file_header() -> Id {
    Id::new("file-header")
}

pub(crate) fn inbox() -> Id {
    Id::new("inbox")
}

pub(crate) fn thread_box() -> Id {
    Id::new("thread-box")
}

pub(crate) fn viewer() -> Id {
    Id::new("viewer")
}

pub(crate) fn viewer_stage() -> Id {
    Id::new("viewer-stage")
}

pub(crate) fn jira_form() -> Id {
    Id::new("jira-form")
}

pub(crate) fn card_box() -> Id {
    Id::new("card-box")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SlotKind {
    Code,
    Fold,
    Removed,
    Rows,
}

impl SlotKind {
    const fn name(self) -> Element {
        Element::new(match self {
            Self::Code => "code",
            Self::Fold => "fold",
            Self::Removed => "removed",
            Self::Rows => "rows",
        })
    }
}

pub(crate) fn slot(path: &RelativePath, piece: SlotKind, at: Count) -> Id {
    Id::new("slot")
        .with(piece.name().as_str())
        .with(in_file(path, at).as_str())
}

pub(crate) fn snippet() -> Id {
    Id::new("snippet")
}

pub(crate) fn body() -> Id {
    Id::new("body")
}

pub(crate) fn pair() -> Id {
    Id::new("pair")
}

pub(crate) fn panel() -> Id {
    Id::new("panel")
}

pub(crate) fn symbols() -> Id {
    Id::new("symbols")
}

pub(crate) fn files() -> Id {
    Id::new("files")
}

pub(crate) fn references() -> Id {
    Id::new("references")
}

pub(crate) fn peek() -> Id {
    Id::new("peek")
}

pub(crate) fn peek_code() -> Id {
    Id::new("peekcode")
}

pub(crate) fn peek_outside() -> Id {
    Id::new("peekout")
}

pub(crate) fn console() -> Id {
    Id::new("console")
}

pub(crate) fn commits() -> Id {
    Id::new("commits")
}

pub(crate) fn source() -> Id {
    Id::new("source")
}

pub(crate) fn search() -> Id {
    Id::new("search")
}

pub(crate) fn changes() -> Id {
    Id::new("changes")
}

pub(crate) fn review() -> Id {
    Id::new("review")
}

pub(crate) fn tooltip_code() -> Id {
    Id::new("tip-code")
}

pub(crate) fn picker() -> Id {
    Id::new("picker")
}

pub(crate) fn palette_box() -> Id {
    Id::new("palette-box")
}

pub(crate) fn settings_box() -> Id {
    Id::new("settings-box")
}

pub(crate) fn draft_box() -> Id {
    Id::new("draft-box")
}

pub(crate) fn removed() -> Id {
    Id::new("removed")
}
