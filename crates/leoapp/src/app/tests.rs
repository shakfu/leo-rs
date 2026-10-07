use super::*;
use crate::keys;

fn app() -> App {
    // a / a1 / a2, b, c
    let mut doc = Document::new_empty("");
    let root = doc.outline().root_position().unwrap();
    doc.set_headline(&root, "a");
    let a1 = doc.outline_mut_untracked().insert_as_last_child(&root);
    doc.set_headline(&a1, "a1");
    let a2 = doc.outline_mut_untracked().insert_as_last_child(&a1);
    doc.set_headline(&a2, "a2");
    let b = doc.outline_mut_untracked().insert_after(&root);
    doc.set_headline(&b, "b");
    let c = doc.outline_mut_untracked().insert_after(&b);
    doc.set_headline(&c, "c");
    doc.clear_undo();
    App::new(doc)
}

/// Type a binding spec, one key at a time, as a terminal would.
fn press(app: &mut App, spec: &str) {
    for key in keys::parse(spec) {
        app.handle_key(KeyEvent::new(key.code, key.mods));
    }
}

fn heads(app: &App) -> Vec<String> {
    app.rows()
        .iter()
        .map(|r| format!("{}{}", "  ".repeat(r.depth), r.headline))
        .collect()
}

#[test]
fn a_folded_subtree_is_not_shown() {
    let mut app = app();
    assert_eq!(heads(&app), vec!["a", "b", "c"]);
    press(&mut app, "l");
    assert_eq!(heads(&app), vec!["a", "  a1", "b", "c"]);
}

#[test]
fn a_two_key_sequence_waits_for_its_second_key() {
    let mut app = app();
    press(&mut app, "j");
    assert_eq!(app.current.h(app.outline()), "b");
    // `g` alone does nothing and waits.
    app.handle_key(KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE));
    assert_eq!(app.current.h(app.outline()), "b");
    assert_eq!(app.pending_keys(), "g");
    app.handle_key(KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE));
    assert_eq!(app.current.h(app.outline()), "a");
    assert_eq!(app.pending_keys(), "");
}

#[test]
fn a_count_repeats_a_command() {
    let mut app = app();
    press(&mut app, "2j");
    assert_eq!(app.current.h(app.outline()), "c");
}

#[test]
fn escape_abandons_a_half_typed_sequence() {
    let mut app = app();
    app.handle_key(KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE));
    app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert_eq!(app.pending_keys(), "");
    // The next `g` starts a fresh sequence rather than completing `gg`.
    app.handle_key(KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE));
    assert_eq!(app.pending_keys(), "g");
}

#[test]
fn an_unbound_key_says_so_and_clears() {
    let mut app = app();
    press(&mut app, "Q");
    assert!(app.message.contains("no binding"), "{}", app.message);
    assert_eq!(app.pending_keys(), "");
}

#[test]
fn the_same_key_means_different_things_in_each_pane() {
    let mut app = app();
    // `j` in the tree moves the selection.
    press(&mut app, "j");
    assert_eq!(app.current.h(app.outline()), "b");
    let p = app.current.clone();
    app.doc.set_body(&p, "one\ntwo\nthree\n");
    press(&mut app, "Tab");
    assert_eq!(app.focus, Focus::Body);
    // `j` in the body moves the text cursor, and leaves the outline alone.
    press(&mut app, "j");
    assert_eq!(app.current.h(app.outline()), "b");
    assert_eq!(app.editor.cursor.0, 1);
}

#[test]
fn shift_tab_moves_between_the_panes_like_tab() {
    // A terminal sends BackTab+SHIFT for it; the table says Shift-Tab.
    let back_tab = KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT);
    let mut app = app();
    app.handle_key(back_tab);
    assert_eq!(app.focus, Focus::Body);
    app.handle_key(back_tab);
    assert_eq!(app.focus, Focus::Tree);
}

#[test]
fn escape_in_the_body_returns_to_the_tree() {
    let mut app = app();
    press(&mut app, "Tab");
    assert_eq!(app.focus, Focus::Body);
    press(&mut app, "Escape");
    assert_eq!(app.focus, Focus::Tree);
}

#[test]
fn leos_literal_chords_act_when_the_terminal_can_send_them() {
    // `examples/keyprobe.rs` shows crossterm decodes `ESC[109;5u` as
    // Char('m')+CONTROL once the enhancement flags are pushed. This is the
    // other half: that such a key reaches Leo's command.
    let ctrl = |c: char| KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL);

    let mut marked = app();
    marked.handle_key(ctrl('m'));
    assert!(
        marked.current.is_marked(marked.outline()),
        "Ctrl-M did not mark"
    );

    let mut cloned = app();
    cloned.handle_key(ctrl('`'));
    assert_eq!(
        heads(&cloned),
        vec!["a", "a", "b", "c"],
        "Ctrl-` did not clone"
    );

    let mut inserted = app();
    inserted.handle_key(ctrl('i'));
    assert_eq!(
        inserted.mode,
        Mode::Headline,
        "Ctrl-I did not insert a node"
    );

    // Ctrl-] demotes the following siblings, Ctrl-[ promotes the
    // children back out. `a` already has one child, which comes with them.
    let mut moved = app();
    moved.handle_key(ctrl(']'));
    assert_eq!(heads(&moved), vec!["a", "  a1", "  b", "  c"]);
    moved.handle_key(ctrl('['));
    assert_eq!(heads(&moved), vec!["a", "a1", "b", "c"]);
}

#[test]
fn leos_shift_ctrl_z_redoes_in_both_panes() {
    let redo = KeyEvent::new(
        KeyCode::Char('z'),
        KeyModifiers::CONTROL | KeyModifiers::SHIFT,
    );
    let mut app = app();
    press(&mut app, "J");
    assert_eq!(heads(&app), vec!["b", "a", "c"]);
    press(&mut app, "u");
    assert_eq!(heads(&app), vec!["a", "b", "c"]);
    app.handle_key(redo);
    assert_eq!(heads(&app), vec!["b", "a", "c"]);

    // The body has its own grammar, and must agree.
    let mut app = body_app("one\ntwo\n");
    press(&mut app, "dd");
    assert_eq!(body(&app), "two\n");
    press(&mut app, "u");
    assert_eq!(body(&app), "one\ntwo\n");
    app.handle_key(redo);
    assert_eq!(body(&app), "two\n");
}

#[test]
fn leos_shift_arrows_move_the_node() {
    let mut app = app();
    press(&mut app, "Shift-Down");
    assert_eq!(heads(&app), vec!["b", "a", "c"]);
    press(&mut app, "Shift-Up");
    assert_eq!(heads(&app), vec!["a", "b", "c"]);
}

#[test]
fn typing_a_headline_lands_in_the_model() {
    let mut app = app();
    press(&mut app, "e");
    assert_eq!(app.mode, Mode::Headline);
    app.handle_key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE));
    for ch in "xyz".chars() {
        app.handle_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
    }
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(app.current.h(app.outline()), "xyz");
    assert_eq!(app.mode, Mode::Normal);
}

#[test]
fn escape_commits_a_body_edit() {
    // Design Q2: Escape commits, and undo is what takes it back.
    let mut app = app();
    press(&mut app, "Tab");
    press(&mut app, "i");
    assert_eq!(app.mode, Mode::Insert);
    for ch in "hello".chars() {
        app.handle_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
    }
    app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert_eq!(app.current.b(app.outline()), "hello\n");
    assert_eq!(app.mode, Mode::Normal);
    press(&mut app, "u");
    assert_eq!(app.current.b(app.outline()), "");
}

#[test]
fn ctrl_c_keeps_a_body_edit_and_asks_to_quit() {
    let mut app = app();
    press(&mut app, "Tab");
    press(&mut app, "i");
    type_text(&mut app, "hello");
    press(&mut app, "Ctrl-c");
    assert_eq!(app.current.b(app.outline()), "hello\n");
    assert_eq!(app.mode, Mode::Confirm);
    assert!(!app.quit);
    // A second Ctrl-c answers the question no; nothing is lost.
    press(&mut app, "Ctrl-c");
    assert_eq!(app.mode, Mode::Normal);
    assert!(!app.quit);
    press(&mut app, "u");
    assert_eq!(app.current.b(app.outline()), "");
}

#[test]
fn ctrl_c_keeps_a_headline_edit() {
    let mut app = app();
    press(&mut app, "e");
    type_text(&mut app, "x");
    press(&mut app, "Ctrl-c");
    assert_eq!(app.current.h(app.outline()), "ax");
    assert_eq!(app.mode, Mode::Confirm);
}

#[test]
fn ctrl_c_quits_at_once_with_nothing_unsaved() {
    let mut app = app();
    app.doc.outline_mut_untracked().changed = false;
    press(&mut app, "Ctrl-c");
    assert!(app.quit);
}

#[test]
fn the_z_family_folds() {
    let mut app = app();
    press(&mut app, "zR");
    assert_eq!(heads(&app), vec!["a", "  a1", "    a2", "b", "c"]);
    press(&mut app, "zM");
    assert_eq!(heads(&app), vec!["a", "b", "c"]);
    press(&mut app, "z2");
    assert_eq!(heads(&app), vec!["a", "  a1", "b", "c"]);
}

#[test]
fn marks_can_be_walked_and_cleared() {
    let mut app = app();
    press(&mut app, "m");
    press(&mut app, "2j");
    press(&mut app, "m");
    press(&mut app, "]m");
    assert_eq!(app.current.h(app.outline()), "a");
    press(&mut app, "M");
    assert!(app.message.contains("unmarked 2"), "{}", app.message);
}

#[test]
fn the_help_overlay_takes_the_keyboard_and_gives_it_back() {
    let mut app = app();
    press(&mut app, "F1");
    assert_eq!(app.mode, Mode::Help);
    // `j` scrolls the help, not the outline.
    press(&mut app, "j");
    assert_eq!(app.help_scroll, 1);
    assert_eq!(app.current.h(app.outline()), "a");
    press(&mut app, "q");
    assert_eq!(app.mode, Mode::Normal);
}

#[test]
fn quitting_with_unsaved_changes_asks_first() {
    let mut app = app();
    let p = app.current.clone();
    app.doc.set_headline(&p, "changed");
    press(&mut app, "q");
    assert_eq!(app.mode, Mode::Confirm);
    assert!(!app.quit);
    app.handle_key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE));
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(!app.quit);
    press(&mut app, "q");
    app.handle_key(KeyEvent::new(KeyCode::Char('y'), KeyModifiers::NONE));
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(app.quit);
}

#[test]
fn quitting_with_an_unwritten_external_file_asks_first_after_a_save() {
    let dir = std::env::temp_dir().join(format!("leotui-unwritten-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let leo = dir.join("u.leo");
    let mut doc = Document::new_empty(leo.to_str().unwrap());
    let root = doc.outline().root_position().unwrap();
    doc.set_headline(&root, "@file u.py");
    doc.set_body(&root, "x = 1\n");
    let mut app = App::new(doc);
    app.run_command_line("write-outline-only");
    assert!(!dir.join("u.py").exists());
    assert!(!app.outline().changed, "{}", app.message);
    press(&mut app, "q");
    assert_eq!(app.mode, Mode::Confirm);
    assert!(!app.quit);
    assert!(
        app.mini_label().contains("1 external file not written"),
        "{}",
        app.mini_label()
    );
    press(&mut app, "Escape");
    // `:e` refuses for the same reason.
    press(&mut app, ":");
    type_text(&mut app, "e elsewhere.leo");
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(app.message.contains("not written"), "{}", app.message);
    // Once written, nothing is lost.
    app.write_dirty_at_file_nodes();
    press(&mut app, "q");
    std::fs::remove_dir_all(&dir).unwrap();
    assert!(app.quit, "{}", app.message);
}

/// An unsaved app in `dir` with `@file good.py` and `@file bad.py`, both
/// dirty. bad.py has a child its body never includes, so it cannot be
/// written until `@others` is added.
fn good_and_bad(dir: &std::path::Path) -> App {
    std::fs::create_dir_all(dir).unwrap();
    let leo = dir.join("s.leo");
    let mut doc = Document::new_empty(leo.to_str().unwrap());
    let bad = doc.outline().root_position().unwrap();
    doc.set_headline(&bad, "@file bad.py");
    doc.set_body(&bad, "x = 1\n");
    let child = doc.outline_mut_untracked().insert_as_last_child(&bad);
    doc.set_headline(&child, "orphan");
    doc.set_body(&child, "y = 2\n");
    let good = doc.outline_mut_untracked().insert_after(&bad);
    doc.set_headline(&good, "@file good.py");
    doc.set_body(&good, "z = 3\n");
    App::new(doc)
}

#[test]
fn save_writes_the_outline_then_every_dirty_file_it_can() {
    let dir = scratch("saveall");
    let mut app = good_and_bad(&dir);
    app.run_command_line("w");
    assert!(dir.join("s.leo").exists(), "{}", app.message);
    assert!(!app.outline().changed);
    assert!(dir.join("good.py").exists(), "{}", app.message);
    assert!(!dir.join("bad.py").exists());
    assert!(
        app.message
            .starts_with("saved s.leo; wrote 1, 1 failed: orphan node"),
        "{}",
        app.message
    );

    // The failed file stays dirty, so quitting still asks.
    let bad = app.outline().root_position().unwrap();
    assert!(bad.is_dirty(app.outline()));
    press(&mut app, "q");
    assert!(
        app.mini_label().contains("1 external file not written"),
        "{}",
        app.mini_label()
    );
    press(&mut app, "Escape");

    // Once the bug is fixed, the next save writes it.
    app.doc.set_body(&bad, "x = 1\n@others\n");
    press(&mut app, "Ctrl-s");
    let written = std::fs::read_to_string(dir.join("bad.py")).unwrap_or_default();
    std::fs::remove_dir_all(&dir).unwrap();
    assert!(written.contains("y = 2"), "{}", app.message);
    assert!(!bad.is_dirty(app.outline()));
}

#[test]
fn a_leo_file_that_fails_to_save_holds_back_every_file() {
    let dir = scratch("leofails");
    let mut app = good_and_bad(&dir);
    app.doc.outline_mut_untracked().file_name =
        dir.join("missing/s.leo").to_string_lossy().to_string();
    app.run_command_line("w");
    let wrote = dir.join("good.py").exists();
    std::fs::remove_dir_all(&dir).unwrap();
    assert!(!wrote, "{}", app.message);
    assert!(app.message.starts_with("save failed: "), "{}", app.message);
    assert!(
        app.message.ends_with("; 2 external files not written"),
        "{}",
        app.message
    );
    assert!(app.outline().changed);
}

#[test]
fn write_outline_only_leaves_the_files_alone() {
    let dir = scratch("outlineonly");
    let mut app = good_and_bad(&dir);
    app.run_command_line("write-outline-only");
    let wrote_leo = dir.join("s.leo").exists();
    let wrote_file = dir.join("good.py").exists();
    std::fs::remove_dir_all(&dir).unwrap();
    assert!(wrote_leo, "{}", app.message);
    assert!(!wrote_file);
}

#[test]
fn declining_to_overwrite_the_leo_file_writes_no_file() {
    let dir = scratch("declineleo");
    let mut app = good_and_bad(&dir);
    app.run_command_line("w");
    let good = app
        .outline()
        .root_position()
        .unwrap()
        .next(app.outline())
        .unwrap();
    app.doc.set_body(&good, "z = 4\n");
    let leo = dir.join("s.leo");
    let mut text = std::fs::read_to_string(&leo).unwrap();
    text.push('\n');
    std::fs::write(&leo, &text).unwrap();
    app.run_command_line("w");
    assert_eq!(app.mode, Mode::Confirm);
    type_text(&mut app, "n");
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    let leo_after = std::fs::read_to_string(&leo).unwrap();
    let good_after = std::fs::read_to_string(dir.join("good.py")).unwrap();
    std::fs::remove_dir_all(&dir).unwrap();
    assert_eq!(leo_after, text);
    assert!(good_after.contains("z = 3"), "{good_after}");
    assert_eq!(
        app.message,
        "not saved: s.leo; 2 external files not written"
    );
}

#[test]
fn save_and_quit_stays_when_the_save_fails() {
    let mut doc = Document::new_empty("/nonexistent-leotui-dir/x.leo");
    let root = doc.outline().root_position().unwrap();
    doc.set_headline(&root, "changed");
    let mut app = App::new(doc);
    app.run_command_line("wq");
    assert!(!app.quit);
    assert!(app.message.contains("save failed"), "{}", app.message);
}

#[test]
fn promote_can_be_undone_and_quit_sees_it() {
    let mut app = app();
    app.doc.outline_mut_untracked().changed = false;
    press(&mut app, "g<");
    assert_eq!(heads(&app), vec!["a", "a1", "b", "c"]);
    press(&mut app, "q");
    assert_eq!(app.mode, Mode::Confirm);
    press(&mut app, "Escape");
    press(&mut app, "u");
    let o = app.outline();
    let root = o.root_position().unwrap();
    assert_eq!(root.num_children(o), 1);
    assert_eq!(o.all_positions().len(), 5);
}

/// An app over `@file x.py`, written, saved, and opened again from `dir`.
fn on_disk(dir: &std::path::Path) -> (App, std::path::PathBuf) {
    std::fs::create_dir_all(dir).unwrap();
    let leo = dir.join("x.leo");
    let mut doc = Document::new_empty(leo.to_str().unwrap());
    let root = doc.outline().root_position().unwrap();
    doc.set_headline(&root, "@file x.py");
    doc.set_body(&root, "x = 1\n");
    doc.write_external_files(false);
    doc.save("").unwrap();
    let app = App::new(Document::open(leo.to_str().unwrap(), true).unwrap());
    (app, dir.join("x.py"))
}

fn scratch(name: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("leotui-{name}-{}", std::process::id()))
}

#[test]
fn writing_over_a_file_changed_on_disk_asks_first() {
    let dir = scratch("changed");
    let (mut app, py) = on_disk(&dir);
    let theirs = std::fs::read_to_string(&py)
        .unwrap()
        .replace("x = 1", "x = 1  # theirs");
    std::fs::write(&py, &theirs).unwrap();
    let root = app.current.clone();
    app.doc.set_body(&root, "x = 2\n");
    app.write_dirty_at_file_nodes();
    assert_eq!(app.mode, Mode::Confirm);
    assert!(
        app.mini_label().contains("changed on disk"),
        "{}",
        app.mini_label()
    );
    type_text(&mut app, "n");
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(std::fs::read_to_string(&py).unwrap(), theirs);
    app.write_dirty_at_file_nodes();
    type_text(&mut app, "y");
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    let written = std::fs::read_to_string(&py).unwrap();
    std::fs::remove_dir_all(&dir).unwrap();
    assert!(written.contains("x = 2"), "{written}");
}

#[test]
fn refresh_from_disk_reads_the_file_and_asks_over_unwritten_edits() {
    let dir = scratch("refresh");
    let (mut app, py) = on_disk(&dir);
    let theirs = std::fs::read_to_string(&py)
        .unwrap()
        .replace("x = 1", "x = 3");
    std::fs::write(&py, theirs).unwrap();
    app.check_disk();
    assert!(
        app.message.contains("changed on disk: x.py"),
        "{}",
        app.message
    );

    let root = app.current.clone();
    app.doc.set_body(&root, "x = 2\n");
    app.run_command_line("refresh-from-disk");
    assert_eq!(app.mode, Mode::Confirm);
    press(&mut app, "Escape");
    assert_eq!(app.current.b(app.outline()), "x = 2\n");

    app.run_command_line("refresh-from-disk");
    type_text(&mut app, "y");
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    std::fs::remove_dir_all(&dir).unwrap();
    assert_eq!(app.current.b(app.outline()), "x = 3\n", "{}", app.message);
}

#[test]
fn e_bang_reverts_to_the_saved_outline() {
    let dir = scratch("revert");
    let (mut app, _) = on_disk(&dir);
    let root = app.current.clone();
    app.doc.set_headline(&root, "@file y.py");
    app.run_command_line("e");
    assert!(app.message.contains("needs a file name") || app.message.contains("unsaved"));
    app.run_command_line("e!");
    std::fs::remove_dir_all(&dir).unwrap();
    assert_eq!(
        app.current.h(app.outline()),
        "@file x.py",
        "{}",
        app.message
    );
    assert!(!app.outline().changed);
}

#[test]
fn w_with_a_path_writes_a_copy_and_refuses_an_existing_file() {
    let dir = scratch("saveto");
    let (mut app, py) = on_disk(&dir);
    let name = app.outline().file_name.clone();
    let copy = dir.join("copy.leo");
    app.run_command_line(&format!("w {}", copy.display()));
    assert!(copy.exists(), "{}", app.message);
    assert_eq!(app.outline().file_name, name);

    let before = std::fs::read_to_string(&py).unwrap();
    app.run_command_line(&format!("w {}", py.display()));
    assert!(
        app.message.contains("add ! to overwrite"),
        "{}",
        app.message
    );
    assert_eq!(std::fs::read_to_string(&py).unwrap(), before);

    app.run_command_line(&format!("saveas {}", copy.display()));
    assert!(app.message.contains("add !"), "{}", app.message);
    app.run_command_line(&format!("saveas! {}", copy.display()));
    let moved = app.outline().file_name.clone();
    std::fs::remove_dir_all(&dir).unwrap();
    assert!(moved.ends_with("copy.leo"), "{moved}");
}

#[test]
fn a_paste_is_text_and_never_keys() {
    let mut app = app();
    press(&mut app, "e");
    app.handle_key(KeyEvent::new(KeyCode::End, KeyModifiers::NONE));
    app.handle_paste(" pasted\ndd");
    assert_eq!(app.mode, Mode::Headline);
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(heads(&app), vec!["a pasted dd", "b", "c"]);

    // In body NORMAL it goes in at the cursor, as one undo.
    press(&mut app, "Tab");
    app.handle_paste("one\n\ttwo\n");
    assert_eq!(app.mode, Mode::Normal);
    assert_eq!(app.current.b(app.outline()), "one\n\ttwo\n\n");
    press(&mut app, "u");
    assert_eq!(app.current.b(app.outline()), "");
}

#[test]
fn writing_over_an_unread_file_asks_first() {
    let dir = std::env::temp_dir().join(format!("leotui-overwrite-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("plain.py");
    let mine = "print('mine')\n";
    std::fs::write(&file, mine).unwrap();
    let mut doc = Document::new_empty("");
    let root = doc.outline().root_position().unwrap();
    doc.set_headline(&root, &format!("@file {}", file.display()));
    doc.set_body(&root, "print('ours')\n");
    let mut app = App::new(doc);

    app.write_dirty_at_file_nodes();
    assert_eq!(app.mode, Mode::Confirm);
    assert!(
        app.mini_label().contains("plain.py"),
        "{}",
        app.mini_label()
    );
    type_text(&mut app, "n");
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(std::fs::read_to_string(&file).unwrap(), mine);
    assert_eq!(app.message, "not overwritten: 1 file");

    app.write_dirty_at_file_nodes();
    type_text(&mut app, "y");
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    let written = std::fs::read_to_string(&file).unwrap();
    std::fs::remove_dir_all(&dir).unwrap();
    assert!(written.contains("print('ours')"), "{written}");
    assert!(written.contains("@+leo"), "{written}");
    assert_eq!(app.message, "wrote 1");
    assert_eq!(app.mode, Mode::Normal);
}

#[test]
fn import_at_file_asks_before_adding_sentinels() {
    let dir = std::env::temp_dir().join(format!("leotui-import-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("x.py");
    std::fs::write(&file, "#!/bin/sh\nx = 1\n").unwrap();
    let mut app = app();
    press(&mut app, ":");
    type_text(&mut app, &format!("import-at-file {}", file.display()));
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(app.mode, Mode::Confirm, "{}", app.message);
    assert!(app.current.h(app.outline()).starts_with("@file "));
    type_text(&mut app, "y");
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    let written = std::fs::read_to_string(&file).unwrap();
    std::fs::remove_dir_all(&dir).unwrap();
    assert!(
        written.starts_with("#!/bin/sh\n# @+leo-ver=5-thin\n"),
        "{written}"
    );
    assert_eq!(app.message, "wrote 1");
}

#[test]
fn the_breadcrumb_names_the_path_to_the_node() {
    let mut app = app();
    press(&mut app, "l");
    press(&mut app, "l");
    assert_eq!(app.current.h(app.outline()), "a1");
    assert_eq!(app.breadcrumb(), "a > a1");
}

/// Type a literal string into the minibuffer.
fn type_text(app: &mut App, text: &str) {
    for ch in text.chars() {
        app.handle_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
    }
}

#[test]
fn the_command_line_runs_a_leo_command_by_name() {
    let mut app = app();
    press(&mut app, ":");
    assert_eq!(app.mode, Mode::Command);
    type_text(&mut app, "move-outline-down");
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(heads(&app), vec!["b", "a", "c"]);
    assert_eq!(app.mode, Mode::Normal);
}

#[test]
fn the_command_line_takes_vim_spellings() {
    let mut app = app();
    let p = app.current.clone();
    app.doc.set_headline(&p, "changed");
    press(&mut app, ":");
    type_text(&mut app, "q");
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    // `:q` refuses while there are unsaved changes.
    assert!(!app.quit);
    assert_eq!(app.mode, Mode::Confirm);
    app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    press(&mut app, ":");
    type_text(&mut app, "q!");
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(app.quit);
}

#[test]
fn an_unknown_command_says_so() {
    let mut app = app();
    press(&mut app, ":");
    type_text(&mut app, "not-a-command");
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(app.message.contains("no such command"), "{}", app.message);
}

#[test]
fn tab_completes_a_command_name() {
    let mut app = app();
    press(&mut app, ":");
    // `unmark-` alone is also `unmark-node-and-parents`.
    type_text(&mut app, "unmark-a");
    app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    assert_eq!(app.mini.as_ref().unwrap().buffer, "unmark-all");
}

#[test]
fn the_command_line_remembers_what_was_typed() {
    let mut app = app();
    press(&mut app, ":");
    type_text(&mut app, "undo");
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    press(&mut app, ":");
    app.handle_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
    assert_eq!(app.mini.as_ref().unwrap().buffer, "undo");
}

#[test]
fn a_number_selects_that_visible_row() {
    let mut app = app();
    press(&mut app, ":");
    type_text(&mut app, "3");
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(app.current.h(app.outline()), "c");
}

#[test]
fn search_moves_as_it_is_typed_and_escape_puts_it_back() {
    let mut app = app();
    press(&mut app, "/");
    assert_eq!(app.mode, Mode::Search);
    type_text(&mut app, "c");
    assert_eq!(app.current.h(app.outline()), "c");
    app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert_eq!(app.current.h(app.outline()), "a");
}

#[test]
fn enter_keeps_the_match_and_n_repeats_it() {
    let mut app = app();
    // Two nodes match "a": the root and a1.
    press(&mut app, "zR");
    press(&mut app, "/");
    type_text(&mut app, "a1");
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(app.current.h(app.outline()), "a1");
    press(&mut app, "n");
    // Only one node matches, so `n` wraps back to it.
    assert_eq!(app.current.h(app.outline()), "a1");
}

#[test]
fn search_is_case_insensitive_until_a_capital_is_typed() {
    let mut app = app();
    let p = app.current.clone();
    app.doc.set_headline(&p, "Alpha");
    press(&mut app, "/");
    type_text(&mut app, "alpha");
    assert_eq!(app.current.h(app.outline()), "Alpha");
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    press(&mut app, "/");
    type_text(&mut app, "ALPHA");
    assert!(app.message.contains("not found"), "{}", app.message);
}

#[test]
fn escape_puts_back_the_line_and_the_theme_a_preview_replaced() {
    let mut app = app();
    press(&mut app, ":");
    app.theme_names = std::rc::Rc::new(
        ["onedark", "onedarker"]
            .iter()
            .map(|s| s.to_string())
            .collect(),
    );
    type_text(&mut app, "theme one");
    app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    assert_eq!(app.mini.as_ref().unwrap().buffer, "theme onedark");
    app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert!(app.mini.is_none());
    // Neither theme is on this machine's disk, so the built-in stands.
    assert_eq!(app.theme.name(), "builtin");
}

#[test]
fn tab_completes_a_theme_name_without_disturbing_the_command() {
    let mut app = app();
    press(&mut app, ":");
    app.theme_names = std::rc::Rc::new(vec!["onelight".to_string()]);
    type_text(&mut app, "theme onel");
    app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    assert_eq!(app.mini.as_ref().unwrap().buffer, "theme onelight");
}

#[test]
fn the_arrows_move_the_drop_down_and_otherwise_walk_the_history() {
    let mut app = app();
    app.command_history = vec!["save".to_string()];
    // No drop-down: Up is the history, as it was.
    press(&mut app, ":");
    app.handle_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
    assert_eq!(app.mini.as_ref().unwrap().buffer, "save");
    app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));

    // A drop-down: Up and Down move through it instead.
    press(&mut app, ":");
    app.theme_names = std::rc::Rc::new(
        ["onedark", "onelight"]
            .iter()
            .map(|s| s.to_string())
            .collect(),
    );
    type_text(&mut app, "theme one");
    app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    assert_eq!(app.mini.as_ref().unwrap().buffer, "theme onedark");
    app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    assert_eq!(app.mini.as_ref().unwrap().buffer, "theme onelight");
    app.handle_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
    assert_eq!(app.mini.as_ref().unwrap().buffer, "theme onedark");
}

#[test]
fn a_theme_that_does_not_load_is_not_saved() {
    // `main` sets the path; an `App` a test builds has none.
    assert!(app().config_path.is_none());
    let path = std::env::temp_dir()
        .join(format!("leotui-app-{}", std::process::id()))
        .join("config.toml");
    let mut app = app();
    app.config_path = Some(path.clone());
    app.run_command_line("theme no-such-theme-anywhere");
    assert!(app.message.contains("not found"), "{}", app.message);
    assert!(!path.exists(), "a failed :theme wrote the settings file");
}

#[test]
fn a_file_that_was_not_read_is_reported() {
    use leolib::external::{FileReport, ReadResult};
    let failure = |path: &str| FileReport {
        headline: format!("@file {path}"),
        path: path.to_string(),
        error: leolib::Error::NotAnExternalFile {
            path: path.to_string(),
        },
    };
    assert_eq!(read_report_message(&ReadResult::default()), None);
    let one = ReadResult {
        errors: vec![failure("a.py")],
        ..Default::default()
    };
    assert_eq!(
        read_report_message(&one).as_deref(),
        Some("external file not read: not a valid external file: a.py")
    );
    let two = ReadResult {
        errors: vec![failure("a.py"), failure("b.py")],
        ..Default::default()
    };
    assert_eq!(
        read_report_message(&two).as_deref(),
        Some("2 external files not read; first: not a valid external file: a.py")
    );
}

#[test]
fn set_colors_takes_the_three_depths_and_refuses_the_rest() {
    let mut app = app();
    app.run_command_line("set colors=16");
    assert_eq!(app.depth, crate::theme::Depth::Ansi16);
    app.run_command_line("set colors=true");
    assert_eq!(app.depth, crate::theme::Depth::True);
    app.run_command_line("set colors=lots");
    assert!(
        app.message.contains("must be true, 256 or 16"),
        "{}",
        app.message
    );
    // A refused value leaves the depth alone.
    assert_eq!(app.depth, crate::theme::Depth::True);
}

#[test]
fn theme_names_the_current_one_and_reports_a_missing_one() {
    let mut app = app();
    app.run_command_line("theme");
    assert!(app.message.contains("builtin"), "{}", app.message);
    app.run_command_line("theme no-such-theme-anywhere");
    assert!(app.message.contains("not found"), "{}", app.message);
    // The theme that was working stays working.
    assert_eq!(app.theme.name(), "builtin");
}

#[test]
fn set_search_headlines_leaves_bodies_out() {
    let mut app = app();
    let p = app.rows()[2].position.clone();
    app.doc.set_body(&p, "a needle\n");
    press(&mut app, "/");
    type_text(&mut app, "needle");
    assert_eq!(app.current.h(app.outline()), "c");
    app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    app.run_command_line("set search=headlines");
    press(&mut app, "/");
    type_text(&mut app, "needle");
    assert!(app.message.contains("not found"), "{}", app.message);
}

#[test]
fn a_search_from_the_outline_lands_in_a_body_and_n_steps_through() {
    let mut app = app();
    let rows = app.rows();
    let (b, c) = (rows[1].position.clone(), rows[2].position.clone());
    app.doc.set_body(&b, "one needle\n");
    app.doc.set_body(&c, "needle\nno\nneedle needle\n");
    press(&mut app, "/");
    type_text(&mut app, "needle");
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(app.current.h(app.outline()), "b");
    assert_eq!(app.focus, Focus::Body);
    assert_eq!(app.editor.cursor, (0, 4));
    assert!(app.hlsearch.is_some());
    press(&mut app, "n");
    assert_eq!(app.current.h(app.outline()), "c");
    assert_eq!(app.editor.cursor, (0, 0));
    press(&mut app, "n");
    assert_eq!(app.editor.cursor, (2, 0));
    press(&mut app, "n");
    assert_eq!(app.editor.cursor, (2, 7));
    press(&mut app, "n");
    assert_eq!(app.current.h(app.outline()), "b");
    assert_eq!(app.message, "search hit BOTTOM, continuing at TOP");
    press(&mut app, "N");
    assert_eq!(app.current.h(app.outline()), "c");
    assert_eq!(app.editor.cursor, (2, 7));
    assert_eq!(app.message, "search hit TOP, continuing at BOTTOM");
    app.run_command_line("noh");
    assert!(app.hlsearch.is_none());
    press(&mut app, "n");
    assert!(app.hlsearch.is_some(), "n highlights again after :noh");
}

#[test]
fn escape_puts_back_the_node_the_pane_and_the_highlight() {
    let mut app = app();
    let c = app.rows()[2].position.clone();
    app.doc.set_body(&c, "a needle\n");
    press(&mut app, "/");
    type_text(&mut app, "needle");
    assert_eq!(app.current.h(app.outline()), "c");
    assert_eq!(app.focus, Focus::Body);
    assert!(app.hlsearch.is_some());
    app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert_eq!(app.current.h(app.outline()), "a");
    assert_eq!(app.focus, Focus::Tree);
    assert!(app.hlsearch.is_none());
}

#[test]
fn set_changes_the_split_and_refuses_nonsense() {
    let mut app = app();
    app.run_command_line("set split=30");
    assert_eq!(app.tree_percent, 30);
    app.run_command_line("set split=nope");
    assert!(app.message.contains("not a number"), "{}", app.message);
    app.run_command_line("set frobnicate");
    assert!(app.message.contains("unknown option"), "{}", app.message);
}

#[test]
fn set_takes_several_options_and_shows_values() {
    let mut app = app();
    app.run_command_line("set nowrap number split:40");
    assert!(app.options.number && !app.options.wrap);
    assert_eq!(app.tree_percent, 40);
    app.run_command_line("set split?");
    assert_eq!(app.message, "split=40");
    app.run_command_line("set split");
    assert_eq!(app.message, "split=40");
    app.run_command_line("set nonumber bogus wrap");
    assert!(!app.options.number);
    assert!(!app.options.wrap, "an error stops the rest");
    assert!(
        app.message.contains("unknown option: bogus"),
        "{}",
        app.message
    );
    app.run_command_line("set");
    assert!(
        app.message
            .starts_with("search=all  split=40  nowrap  nonumber"),
        "{}",
        app.message
    );
}

#[test]
fn substitute_changes_the_body_as_one_undoable_step() {
    let mut app = app();
    let p = app.current.clone();
    app.doc.set_body(&p, "one fish\ntwo fish\n");
    app.doc.clear_undo();
    // Typed, as a user would: `%` and `/` must reach the line intact.
    press(&mut app, ":");
    type_text(&mut app, "%s/fish/cat/");
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(app.current.b(app.outline()), "one cat\ntwo cat\n");
    assert_eq!(app.message, "2 substitutions on 2 lines");
    assert_eq!(app.editor.cursor.0, 1);
    app.run_command_line("undo");
    assert_eq!(app.current.b(app.outline()), "one fish\ntwo fish\n");
    app.run_command_line("s/nothing/x/");
    assert!(app.message.contains("pattern not found"), "{}", app.message);
    app.run_command_line("substitute");
    assert!(app.message.starts_with("usage:"), "{}", app.message);
}

#[test]
fn bufdo_substitutes_in_every_body_as_one_undo_step() {
    let mut app = app();
    let rows = app.rows();
    let (a, b) = (rows[0].position.clone(), rows[1].position.clone());
    app.doc.set_body(&a, "old one\n");
    app.doc.set_body(&b, "old two old\n");
    app.doc.clear_undo();
    app.run_command_line("bufdo %s/old/new/g");
    assert_eq!(a.b(app.outline()), "new one\n");
    assert_eq!(b.b(app.outline()), "new two new\n");
    assert_eq!(app.message, "3 substitutions on 2 lines in 2 nodes");
    app.run_command_line("undo");
    assert_eq!(a.b(app.outline()), "old one\n");
    assert_eq!(b.b(app.outline()), "old two old\n");
    app.run_command_line("bufdo s/old/new/");
    assert!(app.message.contains("range"), "{}", app.message);
    app.run_command_line("bufdo set wrap");
    assert!(app.message.contains("only :s"), "{}", app.message);
    app.run_command_line("bufdo %s/absent/x/");
    assert!(app.message.contains("pattern not found"), "{}", app.message);
}

#[test]
fn bufdo_changes_a_cloned_body_once() {
    let mut app = app();
    let p = app.current.clone();
    app.doc.set_body(&p, "a\n");
    app.doc.clone_node(&p);
    app.run_command_line("bufdo %s/a/aa/");
    assert_eq!(p.b(app.outline()), "aa\n");
    assert_eq!(app.message, "1 substitution on 1 line in 1 node");
}

#[test]
fn ctrl_w_resizes_the_pane_that_has_focus() {
    let mut app = app();
    // Relative to the default, which is a matter of taste.
    let start = app.tree_percent;
    press(&mut app, "Ctrl-w <");
    assert_eq!(app.tree_percent, start - 5, "the outline narrows");
    press(&mut app, "Tab");
    assert_eq!(app.focus, Focus::Body);
    press(&mut app, "Ctrl-w >");
    assert_eq!(app.tree_percent, start - 10, "the body widens");
    press(&mut app, "Ctrl-w <");
    assert_eq!(app.tree_percent, start - 5, "the body narrows");
}

#[test]
fn set_split_saves_the_ratio_and_the_keys_do_not() {
    let dir = std::env::temp_dir().join(format!("leotui-split-{}", std::process::id()));
    let path = dir.join("config.toml");
    let mut app = app();
    app.config_path = Some(path.clone());
    app.run_command_line("set split=30");
    let saved = std::fs::read_to_string(&path).unwrap();
    std::fs::remove_dir_all(&dir).unwrap();
    assert_eq!(saved, "split-ratio = 30\n");
    assert_eq!(app.message, "split: 30% (saved)");
    press(&mut app, "Ctrl-Left");
    assert_eq!(app.tree_percent, 25);
    assert!(!path.exists(), "Ctrl-Left wrote the settings file");
}

#[test]
fn help_for_one_command_names_its_keys() {
    let mut app = app();
    app.run_command_line("help move-outline-down");
    assert!(app.message.contains("move-outline-down"), "{}", app.message);
    assert!(app.message.contains("Shift-Down"), "{}", app.message);
}

#[test]
fn every_shared_binding_reaches_the_body() {
    for b in bindings::BINDINGS
        .iter()
        .filter(|b| b.mode == Mode::Normal && b.focus.is_none())
    {
        let mut app = body_app("one\ntwo\n");
        press(&mut app, b.keys);
        assert_ne!(app.message, "no such command", "{} in the body", b.keys);
    }
}

#[test]
fn f1_opens_help_from_the_body() {
    let mut app = body_app("one\n");
    press(&mut app, "F1");
    assert_eq!(app.mode, Mode::Help);
}

#[test]
fn half_page_down_moves_the_body_cursor() {
    let text: String = (0..100).map(|i| format!("{i}\n")).collect();
    let mut app = body_app(&text);
    app.body_height = 20;
    press(&mut app, "Ctrl-d");
    assert_eq!(app.editor.cursor.0, 10);
    press(&mut app, "3 Ctrl-d");
    assert_eq!(app.editor.cursor.0, 40);
    press(&mut app, "Ctrl-u");
    assert_eq!(app.editor.cursor.0, 30);
    press(&mut app, "20 PageDown");
    assert_eq!(app.editor.cursor.0, 99);
}

#[test]
fn ctrl_w_and_ctrl_u_delete_in_insert() {
    let mut app = body_app("");
    press(&mut app, "i");
    type_text(&mut app, "foo bar");
    press(&mut app, "Ctrl-w");
    type_text(&mut app, "baz");
    press(&mut app, "Escape");
    assert_eq!(body(&app), "foo baz\n");
    // `.` replays what the backspaces left, not the keys.
    press(&mut app, "o");
    type_text(&mut app, "gone");
    press(&mut app, "Ctrl-u");
    type_text(&mut app, "kept");
    press(&mut app, "Escape");
    assert_eq!(body(&app), "foo baz\nkept\n");
    press(&mut app, ".");
    assert_eq!(body(&app), "foo baz\nkept\nkept\n");
}

#[test]
fn a_chord_types_nothing() {
    let mut app = body_app("");
    press(&mut app, "i");
    press(&mut app, "Alt-x");
    press(&mut app, "Ctrl-e");
    // AltGr arrives as Ctrl-Alt on Windows, and types its character.
    app.handle_key(KeyEvent::new(
        KeyCode::Char('@'),
        KeyModifiers::CONTROL | KeyModifiers::ALT,
    ));
    press(&mut app, "Escape");
    assert_eq!(body(&app), "@\n");
}

#[test]
fn ctrl_w_and_ctrl_u_delete_in_a_one_line_input() {
    let mut app = app();
    press(&mut app, ":");
    type_text(&mut app, "abc def");
    press(&mut app, "Ctrl-w");
    assert_eq!(app.mini.as_ref().unwrap().buffer, "abc ");
    press(&mut app, "Alt-x");
    press(&mut app, "Ctrl-u");
    assert_eq!(app.mini.as_ref().unwrap().buffer, "");
}

/// An app whose current node has the given body, focused on it.
fn body_app(text: &str) -> App {
    let mut app = app();
    let p = app.current.clone();
    app.doc.set_body(&p, text);
    app.doc.clear_undo();
    press(&mut app, "Tab");
    app
}

fn body(app: &App) -> String {
    app.current.b(app.outline()).to_string()
}

#[test]
fn an_operator_and_a_motion_delete_a_word() {
    let mut app = body_app("one two three\n");
    press(&mut app, "dw");
    assert_eq!(body(&app), "two three\n");
    assert_eq!(app.editor.register.text, "one ");
}

#[test]
fn a_count_applies_to_the_operator() {
    let mut app = body_app("one two three four\n");
    press(&mut app, "d2w");
    assert_eq!(body(&app), "three four\n");
}

#[test]
fn a_doubled_operator_takes_whole_lines() {
    let mut app = body_app("one\ntwo\nthree\n");
    press(&mut app, "dd");
    assert_eq!(body(&app), "two\nthree\n");
    press(&mut app, "2dd");
    assert_eq!(body(&app), "");
}

#[test]
fn a_text_object_changes_the_word_under_the_cursor() {
    let mut app = body_app("alpha beta gamma\n");
    press(&mut app, "w");
    press(&mut app, "ciw");
    assert_eq!(app.mode, Mode::Insert);
    type_text(&mut app, "BETA");
    app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert_eq!(body(&app), "alpha BETA gamma\n");
}

#[test]
fn a_quoted_object_takes_what_is_inside() {
    let mut app = body_app("say \"hello there\" now\n");
    press(&mut app, "fh");
    press(&mut app, "di\"");
    assert_eq!(body(&app), "say \"\" now\n");
}

#[test]
fn yank_and_put_move_text() {
    let mut app = body_app("one\ntwo\n");
    press(&mut app, "yy");
    press(&mut app, "p");
    assert_eq!(body(&app), "one\none\ntwo\n");
}

#[test]
fn visual_selects_and_an_operator_takes_it() {
    let mut app = body_app("one\ntwo\nthree\n");
    press(&mut app, "V");
    assert_eq!(app.mode, Mode::Visual);
    press(&mut app, "j");
    press(&mut app, "d");
    assert_eq!(body(&app), "three\n");
    assert_eq!(app.mode, Mode::Normal);
}

#[test]
fn escape_leaves_visual_before_it_leaves_the_pane() {
    let mut app = body_app("one\ntwo\n");
    press(&mut app, "v");
    app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert_eq!(app.mode, Mode::Normal);
    assert_eq!(app.focus, Focus::Body);
    app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert_eq!(app.focus, Focus::Tree);
}

#[test]
fn dot_repeats_the_last_change() {
    let mut app = body_app("one two three four\n");
    press(&mut app, "dw");
    assert_eq!(body(&app), "two three four\n");
    press(&mut app, ".");
    assert_eq!(body(&app), "three four\n");
}

#[test]
fn a_count_on_dot_replaces_the_changes_count() {
    // vim: `2dw` then `3.` is `3dw`, one change, undone by one `u`.
    let mut app = body_app("a b c d e f g\n");
    press(&mut app, "2dw");
    assert_eq!(body(&app), "c d e f g\n");
    press(&mut app, "3.");
    assert_eq!(body(&app), "f g\n");
    press(&mut app, "u");
    assert_eq!(body(&app), "c d e f g\n");
}

#[test]
fn a_long_count_in_the_outline_neither_panics_nor_hangs() {
    let mut app = app();
    press(&mut app, "zR");
    press(&mut app, "G");
    let start = std::time::Instant::now();
    press(&mut app, &format!("{}K", "9".repeat(40)));
    assert!(start.elapsed() < std::time::Duration::from_millis(500));
    assert_eq!(app.message, "cannot move up");
}

#[test]
fn dot_repeats_a_change_operator_and_its_text() {
    let mut app = body_app("aa bb\ncc dd\n");
    press(&mut app, "cw");
    type_text(&mut app, "xx");
    app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert_eq!(body(&app), "xx bb\ncc dd\n");
    press(&mut app, "j0");
    press(&mut app, ".");
    assert_eq!(body(&app), "xx bb\nxx dd\n");
}

#[test]
fn one_insert_session_is_one_undo() {
    let mut app = body_app("x\n");
    press(&mut app, "A");
    type_text(&mut app, "yz");
    app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert_eq!(body(&app), "xyz\n");
    press(&mut app, "u");
    assert_eq!(body(&app), "x\n");
}

#[test]
fn a_count_repeats_an_insert() {
    let mut app = body_app("\n");
    press(&mut app, "3i");
    type_text(&mut app, "ab");
    app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert_eq!(body(&app), "ababab\n");
}

#[test]
fn simple_edits_work_and_undo() {
    let mut app = body_app("abc\n");
    press(&mut app, "x");
    assert_eq!(body(&app), "bc\n");
    press(&mut app, "rZ");
    assert_eq!(body(&app), "Zc\n");
    press(&mut app, "~");
    assert_eq!(body(&app), "zc\n");
    press(&mut app, "u");
    press(&mut app, "u");
    press(&mut app, "u");
    assert_eq!(body(&app), "abc\n");
}

#[test]
fn join_puts_one_space_between_the_lines() {
    let mut app = body_app("one\n   two\n");
    press(&mut app, "J");
    assert_eq!(body(&app), "one two\n");
}

#[test]
fn indent_operators_shift_whole_lines() {
    let mut app = body_app("a\nb\n");
    press(&mut app, ">>");
    assert_eq!(body(&app), "    a\nb\n");
    press(&mut app, "<<");
    assert_eq!(body(&app), "a\nb\n");
}

#[test]
fn case_operators_take_a_motion() {
    let mut app = body_app("hello world\n");
    press(&mut app, "gUw");
    assert_eq!(body(&app), "HELLO world\n");
}

#[test]
fn a_body_search_puts_the_cursor_on_the_match() {
    let mut app = body_app("alpha\nbeta\ngamma\n");
    press(&mut app, "/");
    type_text(&mut app, "mm");
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(app.editor.cursor, (2, 2));
    assert_eq!(app.focus, Focus::Body);
    // The outline did not move.
    assert_eq!(app.current.h(app.outline()), "a");
}

#[test]
fn moving_to_another_node_resets_the_cursor() {
    let mut app = body_app("one\ntwo\n");
    press(&mut app, "j");
    assert_eq!(app.editor.cursor.0, 1);
    press(&mut app, "Tab");
    press(&mut app, "j");
    press(&mut app, "Tab");
    assert_eq!(app.editor.cursor, (0, 0));
}

#[test]
fn the_pane_split_can_be_resized() {
    let mut app = app();
    let before = app.tree_percent;
    press(&mut app, "Ctrl-Right");
    assert!(app.tree_percent > before);
    press(&mut app, "Ctrl-Left");
    assert_eq!(app.tree_percent, before);
}

#[test]
fn go_back_and_go_forward_walk_the_selected_nodes() {
    let mut app = app();
    press(&mut app, "j");
    press(&mut app, "j");
    assert_eq!(app.current.h(app.outline()), "c");
    press(&mut app, "H");
    assert_eq!(app.current.h(app.outline()), "b");
    // From the body too, by name: H there is vim's top of the screen.
    press(&mut app, "Tab");
    app.run_command_line("go-back");
    assert_eq!(app.current.h(app.outline()), "a");
    press(&mut app, "Escape");
    press(&mut app, "H");
    assert_eq!(app.message, "no more history");
    press(&mut app, "2 L");
    assert_eq!(app.current.h(app.outline()), "c");
}

#[test]
fn leos_outline_keys_move_and_navigate_as_leo_binds_them() {
    // a / a1 / a2, b, c, with a1 folded: Ctrl-r in the outline moves b under
    // a, as Leo's move-outline-right; Ctrl-l moves it back.
    let mut app = app();
    press(&mut app, "j");
    press(&mut app, "Ctrl-r");
    assert_eq!(heads(&app), ["a", "  a1", "  b", "c"]);
    press(&mut app, "Ctrl-l");
    assert_eq!(heads(&app), ["a", "  a1", "b", "c"]);
    press(&mut app, "Ctrl-d");
    assert_eq!(heads(&app)[2..], ["c", "b"]);
    press(&mut app, "Ctrl-u");
    assert_eq!(heads(&app)[2..], ["b", "c"]);
    // Leo's Alt-arrows navigate; Alt-Shift-arrows move.
    press(&mut app, "Alt-Up");
    assert_eq!(app.current.h(app.outline()), "a1");
    press(&mut app, "Alt-Shift-Left");
    assert_eq!(heads(&app), ["a", "a1", "b", "c"]);
    // Ctrl-} is Leo's other demote, and arrives as a shifted ].
    let mut app = self::app();
    app.handle_key(KeyEvent::new(
        KeyCode::Char('}'),
        KeyModifiers::CONTROL | KeyModifiers::SHIFT,
    ));
    assert_eq!(heads(&app), ["a", "  a1", "  b", "  c"]);
}

#[test]
fn vims_ctrl_keys_keep_their_meaning_in_the_body() {
    let mut app = body_app("one\ntwo\n");
    press(&mut app, "dd");
    press(&mut app, "u");
    assert_eq!(body(&app), "one\ntwo\n");
    press(&mut app, "Ctrl-r");
    assert_eq!(body(&app), "two\n", "Ctrl-r is redo in the body");
}

#[test]
fn cmd_is_leos_ctrl_from_either_pane() {
    let mut app = app();
    press(&mut app, "j");
    press(&mut app, "Tab");
    // Cmd-r, as leogui sends it on macOS, from the body: Leo's
    // move-outline-right, not vim's redo.
    app.handle_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::SUPER));
    assert_eq!(heads(&app), ["a", "  a1", "  b", "c"]);
    // Cmd-z is Leo's undo, as Ctrl-z is.
    app.handle_key(KeyEvent::new(KeyCode::Char('z'), KeyModifiers::SUPER));
    assert_eq!(heads(&app), ["a", "  a1", "b", "c"]);
}

#[test]
fn ctrl_g_cancels_and_ctrl_shift_c_copies_rather_than_interrupts() {
    let mut app = app();
    press(&mut app, ":");
    assert_eq!(app.mode, Mode::Command);
    press(&mut app, "Ctrl-g");
    assert_eq!(app.mode, Mode::Normal);
    let before = app.row_count();
    press(&mut app, "Ctrl-Shift-c");
    press(&mut app, "Ctrl-Shift-v");
    assert_eq!(app.row_count(), before + 1);
    assert_eq!(app.mode, Mode::Normal);
}

#[test]
fn x_then_undo_then_redo_on_a_one_character_line() {
    let mut app = body_app("x\n");
    press(&mut app, "x");
    let after = body(&app);
    press(&mut app, "u");
    assert_eq!(body(&app), "x\n");
    press(&mut app, "Ctrl-r");
    assert_eq!(body(&app), after);
}

#[test]
fn gd_selects_the_section_the_line_names() {
    let mut app = body_app("x = 1\n    << Imports >>\n<< nowhere >>\n");
    let root = app.current.clone();
    let a1 = root.first_child(app.outline()).unwrap();
    // Matched as the writer matches: case and spaces aside.
    app.doc.set_headline(&a1, "<<imports>>");
    press(&mut app, "gd");
    assert_eq!(app.message, "no section reference on this line");
    press(&mut app, "j");
    press(&mut app, "gd");
    assert_eq!(app.current, a1);
    press(&mut app, "Alt-Left");
    assert_eq!(app.current, root);
    assert_eq!(app.focus, Focus::Body);
    // The body cursor is where it was when the node was left.
    assert_eq!(app.editor.cursor.0, 1);
    press(&mut app, "j");
    press(&mut app, "gd");
    assert_eq!(app.message, "undefined section: << nowhere >>");
}

#[test]
fn hoist_shows_one_subtree_and_keeps_the_selection_inside_it() {
    let mut app = app();
    press(&mut app, "zR");
    press(&mut app, "zM");
    press(&mut app, "l");
    press(&mut app, "j");
    assert_eq!(app.current.h(app.outline()), "a1");
    press(&mut app, "zh");
    assert_eq!(heads(&app), vec!["a1", "  a2"]);
    press(&mut app, "k");
    assert_eq!(app.current.h(app.outline()), "a1");
    press(&mut app, "G");
    assert_eq!(app.current.h(app.outline()), "a2");
    press(&mut app, "j");
    assert_eq!(app.current.h(app.outline()), "a2");
    // a2 cannot leave, and the hoisted node cannot move.
    press(&mut app, "<<");
    assert_eq!(app.current.h(app.outline()), "a2");
    assert!(app.message.starts_with("not while hoisted"));
    press(&mut app, "gg");
    press(&mut app, "K");
    assert!(app.message.starts_with("not while hoisted"));
    press(&mut app, "h");
    press(&mut app, "h");
    assert_eq!(app.current.h(app.outline()), "a1");
    // zH puts back the fold hoisting opened.
    press(&mut app, "zH");
    assert_eq!(heads(&app), vec!["a", "  a1", "b", "c"]);
}

#[test]
fn selecting_outside_the_hoist_dehoists() {
    let mut app = app();
    press(&mut app, "zh");
    assert_eq!(heads(&app), vec!["a", "  a1"]);
    press(&mut app, "/");
    type_text(&mut app, "c");
    press(&mut app, "Enter");
    assert_eq!(app.current.h(app.outline()), "c");
    assert!(app.hoist_limit().is_none());
    press(&mut app, "zH");
    assert_eq!(app.message, "not hoisted");
}

#[test]
fn the_marked_node_commands_run_from_the_command_line() {
    let mut app = app();
    press(&mut app, "zR");
    press(&mut app, "m");
    press(&mut app, "G");
    press(&mut app, "m");
    press(&mut app, ":");
    type_text(&mut app, "clone-marked-nodes");
    press(&mut app, "Enter");
    assert_eq!(app.current.h(app.outline()), "Clones of marked nodes");
    // The selection sits under the marked a; deleting a must move it.
    press(&mut app, "gg");
    press(&mut app, "2j");
    assert_eq!(app.current.h(app.outline()), "a2");
    press(&mut app, ":");
    type_text(&mut app, "delete-marked-nodes");
    press(&mut app, "Enter");
    // a and c, and their clones: a clone shares its node's mark.
    assert_eq!(app.message, "deleted 4");
    assert!(app.outline().position_exists(&app.current));
    assert_eq!(heads(&app), vec!["b", "Clones of marked nodes"]);
}

#[test]
fn alt_a_sorts_the_siblings_and_keeps_the_node_selected() {
    let mut app = app();
    let a = app.current.clone();
    app.doc.set_headline(&a, "d");
    press(&mut app, "Alt-a");
    assert_eq!(heads(&app), vec!["b", "c", "d"]);
    assert_eq!(app.current.h(app.outline()), "d");
    press(&mut app, "Alt-a");
    assert_eq!(app.message, "already sorted");
    press(&mut app, "u");
    assert_eq!(heads(&app), vec!["d", "b", "c"]);
}

#[test]
fn a_command_line_ends_visual() {
    let mut app = body_app("one\ntwo\n");
    press(&mut app, "V");
    press(&mut app, ":");
    type_text(&mut app, "noh");
    press(&mut app, "Enter");
    assert_eq!(app.mode, Mode::Normal);
    assert!(app.editor.visual.is_none());
    press(&mut app, "V");
    press(&mut app, ":");
    press(&mut app, "Escape");
    assert!(app.editor.visual.is_none());
}

#[test]
fn extract_takes_the_visual_lines_into_a_child() {
    let mut app = body_app("keep\n<< part >>\ninside\nafter\n");
    press(&mut app, "j");
    press(&mut app, "V");
    press(&mut app, "j");
    press(&mut app, ":");
    type_text(&mut app, "extract");
    press(&mut app, "Enter");
    assert_eq!(app.message, "extracted: << part >>");
    assert_eq!(body(&app), "keep\n<< part >>\nafter\n");
    let child = app.current.first_child(app.outline()).unwrap();
    assert_eq!(child.b(app.outline()), "inside\n");
    assert_eq!(app.mode, Mode::Normal);
    press(&mut app, "u");
    assert_eq!(body(&app), "keep\n<< part >>\ninside\nafter\n");
}

#[test]
fn cfa_clones_the_matches_and_cff_reuses_the_pattern() {
    let mut app = app();
    press(&mut app, ":");
    type_text(&mut app, "cfa a");
    press(&mut app, "Enter");
    assert_eq!(app.message, "found 1 for a");
    assert_eq!(app.current.h(app.outline()), "Found 1:a");
    assert_eq!(app.focus, Focus::Tree);
    press(&mut app, "u");
    assert_eq!(heads(&app), vec!["a", "b", "c"]);
    // No pattern: the last one. Flattened, a1 and a2 count too.
    press(&mut app, ":");
    type_text(&mut app, "cff");
    press(&mut app, "Enter");
    assert_eq!(app.message, "found 3 for a");
}

#[test]
fn reformat_paragraph_wraps_to_the_page_width_and_moves_on() {
    let mut app = body_app("@pagewidth 14\none two three\nfour five six seven\n\nnext\n");
    press(&mut app, "j");
    press(&mut app, ":");
    type_text(&mut app, "reformat-paragraph");
    press(&mut app, "Enter");
    assert_eq!(
        body(&app),
        "@pagewidth 14\none two three\nfour five six\nseven\n\nnext\n"
    );
    assert_eq!(app.editor.cursor, (5, 0));
    press(&mut app, "u");
    assert_eq!(
        body(&app),
        "@pagewidth 14\none two three\nfour five six seven\n\nnext\n"
    );
}

#[test]
fn write_at_file_nodes_writes_a_clean_file_and_w_does_not() {
    let dir = scratch("write-all");
    let (mut app, py) = on_disk(&dir);
    std::fs::write(&py, "x = 1\n").unwrap();
    // Recording the edit as read keeps the changed-on-disk guard out of it.
    let path = py.to_string_lossy().to_string();
    app.doc
        .outline_mut_untracked()
        .record_file_stamp(&path, leolib::util::file_stamp(&path));
    app.write_dirty_at_file_nodes();
    assert_eq!(std::fs::read_to_string(&py).unwrap(), "x = 1\n");
    app.write_at_file_nodes();
    let written = std::fs::read_to_string(&py).unwrap();
    std::fs::remove_dir_all(&dir).unwrap();
    assert!(written.contains("@+leo"), "{written}");
}

#[test]
fn each_node_keeps_its_body_cursor() {
    let mut app = body_app("one\ntwo\nthree\n");
    press(&mut app, "2j");
    press(&mut app, "l");
    press(&mut app, "Tab");
    press(&mut app, "j");
    assert_eq!(app.editor.cursor, (0, 0));
    press(&mut app, "k");
    assert_eq!(app.editor.cursor, (2, 1));
}

#[test]
fn messages_lists_what_the_status_line_showed() {
    let mut app = app();
    press(&mut app, "Q");
    assert_eq!(app.message, "no binding for Q");
    press(&mut app, "j");
    assert!(app.message.is_empty());
    app.run_command_line("messages");
    assert_eq!(app.mode, Mode::Help);
    let (_, lines) = app.overlay.clone().unwrap();
    assert_eq!(lines, vec!["no binding for Q"]);
    press(&mut app, "q");
    assert_eq!(app.mode, Mode::Normal);
    assert!(app.overlay.is_none());
}

#[test]
fn a_file_the_importer_normalized_is_reported() {
    use leolib::external::{FileNote, ReadResult};
    let note = |h: &str| FileNote {
        headline: h.to_string(),
        path: String::new(),
        message: "the text was reformatted by the importer".to_string(),
    };
    let one = ReadResult {
        warnings: vec![note("@auto a.xml")],
        ..Default::default()
    };
    assert_eq!(
        read_report_message(&one).as_deref(),
        Some("@auto a.xml: the text was reformatted by the importer")
    );
    let two = ReadResult {
        warnings: vec![note("@auto a.xml"), note("@auto b.xml")],
        ..Default::default()
    };
    assert!(read_report_message(&two).unwrap().contains(":messages"));
    assert_eq!(read_report_lines(&two).len(), 2);
}

#[test]
fn e_keeps_the_theme_the_register_and_the_last_search() {
    let dir = scratch("e-keeps");
    let (mut app, _) = on_disk(&dir);
    app.depth = crate::theme::Depth::Ansi16;
    press(&mut app, "/");
    type_text(&mut app, "x");
    press(&mut app, "Enter");
    press(&mut app, "Tab");
    press(&mut app, "yy");
    let theme = app.theme.name().to_string();
    app.run_command_line("e!");
    std::fs::remove_dir_all(&dir).unwrap();
    assert!(app.message.starts_with("opened"), "{}", app.message);
    assert_eq!(app.theme.name(), theme);
    assert_eq!(app.depth, crate::theme::Depth::Ansi16);
    assert_eq!(app.last_search.as_ref().unwrap().pattern, "x");
    press(&mut app, "Tab");
    press(&mut app, "p");
    assert_eq!(body(&app), "x = 1\nx = 1\n");
}

#[test]
fn a_search_that_finds_nothing_says_so_on_enter() {
    let mut app = app();
    press(&mut app, "/");
    type_text(&mut app, "zzz");
    press(&mut app, "Enter");
    assert_eq!(app.message, "not found: zzz");
}

#[test]
fn escape_folds_again_what_the_search_preview_unfolded() {
    let mut app = app();
    assert_eq!(heads(&app), vec!["a", "b", "c"]);
    press(&mut app, "/");
    type_text(&mut app, "a2");
    assert_eq!(app.current.h(app.outline()), "a2");
    assert_eq!(heads(&app).len(), 5);
    press(&mut app, "Escape");
    assert_eq!(app.current.h(app.outline()), "a");
    assert_eq!(heads(&app), vec!["a", "b", "c"]);
}

#[test]
fn goto_global_line_and_show_file_line_are_each_others_reverse() {
    let mut app = app();
    let root = app.current.clone();
    app.doc.set_headline(&root, "@clean x.py");
    app.doc.set_body(&root, "import os\n@others\n");
    let a1 = root.first_child(app.outline()).unwrap();
    app.doc.set_body(&a1, "def f():\n    return 1\n");
    app.run_command_line("goto-global-line 3");
    assert_eq!(app.message, "goto-global-line found: 3");
    assert_eq!(app.current, a1);
    assert_eq!(app.focus, Focus::Body);
    assert_eq!(app.editor.cursor, (1, 0));
    app.run_command_line("show-file-line");
    assert_eq!(app.message, "line 3");
    app.run_command_line("goto-global-line 99");
    assert_eq!(app.message, "goto-global-line not found: 99");
}

#[test]
fn move_marked_nodes_moves_them_and_undoes() {
    let mut app = app();
    press(&mut app, "j");
    press(&mut app, "m");
    app.run_command_line("move-marked-nodes");
    assert_eq!(app.message, "moved 1");
    assert_eq!(app.current.h(app.outline()), "Moved marked nodes");
    press(&mut app, "u");
    assert!(heads(&app).iter().all(|h| !h.contains("Moved")));
}

#[test]
fn renaming_a_node_to_at_file_keeps_its_shebang_first() {
    let mut app = app();
    let root = app.current.clone();
    app.doc.set_headline(&root, "@auto x.sh");
    app.doc.set_body(&root, "#!/bin/sh\necho hi\n");
    app.doc.clear_undo();
    press(&mut app, "e");
    for _ in 0.."@auto x.sh".len() {
        press(&mut app, "Backspace");
    }
    type_text(&mut app, "@file x.sh");
    press(&mut app, "Enter");
    assert_eq!(app.message, "added @first to 1 line");
    assert_eq!(body(&app), "@first #!/bin/sh\necho hi\n");
    press(&mut app, "u");
    assert_eq!(app.current.h(app.outline()), "@auto x.sh");
    assert_eq!(body(&app), "#!/bin/sh\necho hi\n");
}

#[test]
fn a_new_node_is_indented_before_it_has_a_headline() {
    // Leo's flow: insert a node, Ctrl-r while its headline is still being
    // typed, and the node moves, headline kept.
    let mut app = app();
    press(&mut app, "j");
    press(&mut app, "o");
    assert_eq!(app.mode, Mode::Headline);
    press(&mut app, "Ctrl-r");
    assert_eq!(app.mode, Mode::Normal);
    let new = app.current.clone();
    assert_eq!(new.level(), 1, "the new node is now a child of b");
    // Cmd-r does the same, as leogui sends it on macOS.
    let mut app = self::app();
    press(&mut app, "j");
    press(&mut app, "o");
    app.handle_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::SUPER));
    assert_eq!(app.current.level(), 1);
}

#[test]
fn ctrl_i_after_a_headline_keeps_it_and_starts_the_next() {
    let mut app = app();
    press(&mut app, "o");
    for _ in 0..app.mini.as_ref().unwrap().buffer.chars().count() {
        press(&mut app, "Backspace");
    }
    press(&mut app, "first");
    app.handle_key(KeyEvent::new(KeyCode::Char('i'), KeyModifiers::CONTROL));
    assert_eq!(app.mode, Mode::Headline);
    assert!(heads(&app).iter().any(|h| h.trim() == "first"));
    // Ctrl-g still abandons the line being typed.
    press(&mut app, "Ctrl-g");
    assert_eq!(app.mode, Mode::Normal);
}

#[test]
fn a_file_changed_on_disk_is_flagged_until_reloaded() {
    let dir = scratch("reload");
    let (mut app, py) = on_disk(&dir);
    let root = app.current.clone();
    assert_eq!(app.file_state(&root), None);
    let theirs = std::fs::read_to_string(&py)
        .unwrap()
        .replace("x = 1", "x = 1  # theirs");
    std::fs::write(&py, theirs).unwrap();
    // Not until the outline looks.
    assert_eq!(app.file_state(&root), None);
    app.check_disk();
    assert_eq!(app.file_state(&root), Some(FileState::ChangedOnDisk));
    assert_eq!(app.files_changed_on_disk().len(), 1);

    app.reload_changed_files();
    let body = app.current.b(app.outline()).to_string();
    std::fs::remove_dir_all(&dir).unwrap();
    assert!(body.contains("# theirs"), "{body}");
    assert!(app.files_changed_on_disk().is_empty());
    assert_eq!(app.file_state(&app.current), None);
}

#[test]
fn keeping_a_changed_file_lets_the_next_write_overwrite_it() {
    let dir = scratch("keep");
    let (mut app, py) = on_disk(&dir);
    let theirs = std::fs::read_to_string(&py)
        .unwrap()
        .replace("x = 1", "x = 1  # theirs");
    std::fs::write(&py, theirs).unwrap();
    app.check_disk();
    let root = app.current.clone();
    app.doc.set_body(&root, "x = 2\n");
    app.keep_changed_files();
    assert_eq!(app.message, "kept the outline's text of 1 file");
    assert_eq!(app.file_state(&root), Some(FileState::Unwritten));
    app.write_dirty_at_file_nodes();
    let written = std::fs::read_to_string(&py).unwrap();
    std::fs::remove_dir_all(&dir).unwrap();
    assert_eq!(app.mode, Mode::Normal);
    assert!(written.contains("x = 2"), "{written}");
    assert_eq!(app.file_state(&root), None);
}

#[test]
fn an_unread_file_and_a_refused_one_are_flagged() {
    let dir = scratch("states");
    std::fs::create_dir_all(&dir).unwrap();
    let exists = dir.join("exists.py");
    std::fs::write(&exists, "print('theirs')\n").unwrap();
    let mut doc = Document::new_empty("");
    let root = doc.outline().root_position().unwrap();
    doc.set_headline(&root, &format!("@file {}", exists.display()));
    let unreadable = doc.outline_mut_untracked().insert_after(&root);
    // A directory where the file should be: the read fails.
    let gone = dir.join("dir.py");
    std::fs::create_dir_all(&gone).unwrap();
    doc.set_headline(&unreadable, &format!("@file {}", gone.display()));
    let mut app = App::new(doc);
    // Never read, so writing it would lose what it holds.
    assert_eq!(app.file_state(&root), Some(FileState::Refused));
    let other = app.row_position(1).unwrap();
    app.select(other.clone());
    app.read_at_file_nodes();
    // A new node is dirty, so the read asks first.
    type_text(&mut app, "y");
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    let rows = app.rows();
    std::fs::remove_dir_all(&dir).unwrap();
    assert!(app.message.contains("not read"), "{}", app.message);
    assert_eq!(rows[0].file_state, Some(FileState::Refused));
    assert_eq!(rows[1].file_state, Some(FileState::Unread));
}

#[test]
fn an_outline_opened_beside_shares_the_session() {
    let dir = scratch("beside");
    std::fs::create_dir_all(&dir).unwrap();
    let mut first = app();
    first.options.number = true;
    first.command_history.push("set number".into());
    first.settings.wrap = Some(true);
    let path = dir.join("other.leo");
    let second = first
        .open_beside(Some(path.to_str().unwrap()), std::sync::Arc::new(|| {}))
        .unwrap();
    std::fs::remove_dir_all(&dir).unwrap();
    assert!(second.options.number);
    assert_eq!(second.command_history, ["set number"]);
    assert_eq!(second.settings.wrap, Some(true));
    assert!(
        second.message.starts_with("new outline: "),
        "{}",
        second.message
    );
    assert!(second.outline().file_name.ends_with("other.leo"));
    // The first outline is untouched.
    assert_eq!(heads(&first)[0], "a");
    let unsaved = first.open_beside(None, std::sync::Arc::new(|| {})).unwrap();
    assert!(unsaved.outline().file_name.is_empty());
    assert!(unsaved.message.is_empty());
}

#[test]
fn opening_another_outline_keeps_the_settings_and_servers() {
    let dir = scratch("reopen");
    std::fs::create_dir_all(&dir).unwrap();
    let mut app = app();
    app.settings.lsp = true;
    app.settings.wrap = Some(true);
    app.settings.servers = vec![leolsp::ServerConfig {
        language: "python".into(),
        command: "pylsp".into(),
    }];
    let woken = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let flag = woken.clone();
    let wake: leolsp::server::Wake =
        std::sync::Arc::new(move || flag.store(true, std::sync::atomic::Ordering::SeqCst));
    let settings = app.settings.clone();
    app.set_lsp(&settings, wake);
    app.theme_setting = "theme-light";
    let path = dir.join("other.leo");
    app.run_command_line(&format!("e! {}", path.display()));
    std::fs::remove_dir_all(&dir).unwrap();
    assert!(
        app.outline().file_name.ends_with("other.leo"),
        "{}",
        app.message
    );
    assert_eq!(app.settings, settings);
    assert_eq!(app.theme_setting, "theme-light");
    // A server is started on first use; the front end's wake goes with it.
    let lsp = app.lsp.as_ref().expect("servers kept");
    (lsp.wake())();
    assert!(woken.load(std::sync::atomic::Ordering::SeqCst));
}

#[test]
fn a_restored_selection_puts_the_cursor_back_within_the_body() {
    let mut app = app();
    let o = app.outline();
    let b = o.all_positions()[3].clone();
    let gnx = b.gnx(o).to_string();
    app.doc.set_body(&b, "one\ntwo\n");
    assert!(app.restore_selection(&gnx, (1, 99)));
    assert_eq!(app.current.h(app.outline()), "b");
    assert_eq!(app.editor.cursor, (1, 2));
    assert_eq!(app.focus, Focus::Tree);
    assert!(!app.restore_selection("nobody", (0, 0)));
}

#[test]
fn a_rows_clone_count_and_its_clones_are_known() {
    let mut app = app();
    app.run("clone-node", 1);
    let rows = app.rows();
    let cloned: Vec<_> = rows.iter().filter(|r| r.cloned).collect();
    assert_eq!(cloned.len(), 2);
    assert!(cloned.iter().all(|r| r.clones == 2));
    let clones = app.clones_of(&app.current);
    assert_eq!(clones.len(), 2);
    assert!(clones.contains(&app.current));
    assert!(rows.iter().filter(|r| !r.cloned).all(|r| r.clones == 1));
}

#[test]
fn a_file_imports_as_an_auto_tree_from_the_command_line() {
    let dir = scratch("import-auto");
    std::fs::create_dir_all(&dir).unwrap();
    let py = dir.join("y.py");
    std::fs::write(&py, "def f():\n    return 1\n").unwrap();
    let mut app = app();
    app.run_command_line(&format!("import-auto {}", py.display()));
    std::fs::remove_dir_all(&dir).unwrap();
    assert!(
        app.current.h(app.outline()).starts_with("@auto "),
        "{}",
        app.message
    );
    assert!(app.message.starts_with("imported @auto"), "{}", app.message);
    // The importer split it: the def is a child.
    assert!(app.current.has_children(app.outline()));
}

#[test]
fn a_long_body_is_coloured_whole_by_a_later_poll() {
    let mut app = app();
    let root = app.outline().root_position().unwrap();
    let body: String = (0..800)
        .map(|i| format!("x{i} = 'text'  # note\n"))
        .collect();
    app.doc
        .set_body(&root, &format!("@language python\n{body}"));
    let screen = crate::view::Viewport { rows: 20, cols: 80 };
    let first = app.body_view(screen);
    let whole = crate::highlight::highlight(&first.lines, "python");
    assert_ne!(*first.spans, whole, "only the screen is coloured at first");
    let start = std::time::Instant::now();
    while !app.poll() {
        assert!(start.elapsed().as_secs() < 10, "never finished");
        assert!(app.poll_after().is_some(), "a front end would not wake");
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert_eq!(*app.body_view(screen).spans, whole);
}

#[test]
fn a_clone_conflict_is_on_the_status_line_and_in_the_log() {
    use leolib::external::{CloneConflict, NodeText, ReadResult};
    let text = |path: &str, body: &str| NodeText {
        path: path.into(),
        headline: "<< example >>".into(),
        body: body.into(),
    };
    let report = ReadResult {
        conflicts: vec![CloneConflict {
            gnx: "a.2".into(),
            old: text("/p/README.md", "new\n"),
            new: text("/p/tests/t.py", "old\n"),
        }],
        ..Default::default()
    };
    assert_eq!(
        super::read_report_message(&report).as_deref(),
        Some("<< example >> differs between external files; both texts are under Recovered Nodes")
    );
    assert_eq!(
        super::read_report_lines(&report),
        vec!["clone conflict: << example >>: /p/README.md and /p/tests/t.py differ; kept /p/tests/t.py"]
    );
}
