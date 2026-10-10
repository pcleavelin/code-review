use crate::wire::{WireKind, parse_diff};

const DIFF: &str = "diff --git a/src/lib.rs b/src/lib.rs
index 1111111..2222222 100644
--- a/src/lib.rs
+++ b/src/lib.rs
@@ -1,3 +1,3 @@ fn main()
 a
-b
+x
 c
@@ -10,0 +11,2 @@
+-- not a header
+
diff --git a/old name.txt b/new name.txt
similarity index 90%
rename from old name.txt
rename to new name.txt
diff --git a/gone.txt b/gone.txt
deleted file mode 100644
--- a/gone.txt
+++ /dev/null
@@ -1 +0,0 @@
-bye
\\ No newline at end of file
diff --git a/image.png b/image.png
new file mode 100644
Binary files /dev/null and b/image.png differ
";

#[test]
fn every_file_of_a_diff_keeps_its_kind_and_paths() {
    let files = parse_diff(DIFF);
    let summary: Vec<(&str, Option<&str>, WireKind)> = files
        .iter()
        .map(|file| (file.path.as_str(), file.old_path.as_deref(), file.kind))
        .collect();
    assert_eq!(
        summary,
        vec![
            ("src/lib.rs", None, WireKind::Modified),
            ("new name.txt", Some("old name.txt"), WireKind::Renamed),
            ("gone.txt", None, WireKind::Deleted),
            ("image.png", None, WireKind::Added),
        ]
    );
}

#[test]
fn hunk_starts_become_zero_based_and_an_empty_side_points_past_its_line() {
    let files = parse_diff(DIFF);
    let lib = &files[0];
    let starts: Vec<(u32, u32)> = lib
        .hunks
        .iter()
        .map(|hunk| (hunk.old_start, hunk.new_start))
        .collect();
    assert_eq!(starts, vec![(0, 0), (10, 10)]);
    let deleted = &files[2];
    assert_eq!(deleted.hunks[0].new_start, 0);
}

#[test]
fn hunk_lines_that_look_like_headers_or_are_blank_stay_in_the_hunk() {
    let files = parse_diff(DIFF);
    let second = &files[0].hunks[1];
    assert_eq!(
        second.lines,
        vec![('+', "-- not a header".to_owned()), ('+', String::new())]
    );
    assert_eq!(files[2].hunks[0].lines, vec![('-', "bye".to_owned())]);
}
