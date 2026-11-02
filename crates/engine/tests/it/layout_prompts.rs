//! LAYOUT / -LAYOUT typed at the command line asks for an option and the layout names it needs
//! instead of running the next inputs as commands.

use cadcraft_doc::Space;
use cadcraft_engine::Session;

fn layouts(s: &Session) -> Vec<String> {
    let mut ls: Vec<_> = s.doc().unwrap().layouts.iter().map(|l| (l.tab_order, l.name.clone())).collect();
    ls.sort();
    ls.into_iter().map(|(_, n)| n).collect()
}

#[test]
fn typed_layout_options_prompt_for_names() {
    let mut s = Session::new();
    assert_eq!(layouts(&s), ["Layout1", "Layout2"]);

    s.cmdline("LAYOUT").unwrap();
    let p = s.current_prompt().expect("LAYOUT prompts").display();
    assert_eq!(p, "Enter layout option or [Copy/Delete/New/Template/Rename/SAveas/Set/?] <set>:");
    s.cmdline("n").unwrap();
    assert_eq!(s.current_prompt().unwrap().display(), "Enter name of new layout <Layout3>:");
    s.cmdline("Sheet9").unwrap();
    assert!(s.current_prompt().is_none());
    assert_eq!(layouts(&s), ["Layout1", "Layout2", "Sheet9"]);

    // Script form, names with spaces, and the current layout as the default.
    s.script("-LAYOUT S Layout2\n-LAYOUT R\nLayout2\nSheet A\n-LAYOUT C\n\n\n").unwrap();
    assert_eq!(s.layout_space(), Space::Paper("Sheet A".into()));
    assert_eq!(layouts(&s), ["Layout1", "Sheet A", "Sheet9", "Sheet A (2)"]);
    s.script("-LAYOUT D Sheet9\n").unwrap();
    assert_eq!(layouts(&s), ["Layout1", "Sheet A", "Sheet A (2)"]);

    // An invalid name re-prompts; Enter at the option prompt is Set.
    s.cmdline("LAYOUT N").unwrap();
    s.cmdline("Layout1").unwrap();
    assert!(s.current_prompt().is_some_and(|p| p.message == "Enter name of new layout"));
    s.input(cadcraft_engine::Input::Cancel).unwrap();
    s.script("LAYOUT\n\nModel\n").unwrap();
    assert_eq!(s.layout_space(), Space::Model);

    // ? lists; Template reports and ends without taking the next input.
    let n = s.log.len();
    s.cmdline("LAYOUT ? Sheet*").unwrap();
    assert!(s.log[n..].iter().any(|l| l == "Layout: Sheet A (2)"), "{:?}", &s.log[n..]);
    assert!(!s.log[n..].iter().any(|l| l == "Layout: Layout1"));
    let n = s.log.len();
    s.cmdline("LAYOUT T").unwrap();
    assert!(s.current_prompt().is_none());
    assert!(s.log[n..].iter().any(|l| l.contains("not available yet")), "{:?}", &s.log[n..]);
    assert_eq!(layouts(&s), ["Layout1", "Sheet A", "Sheet A (2)"]);

    // The whole command is one undo step.
    s.cmdline("LAYOUT N Sheet7").unwrap();
    s.undo().unwrap();
    assert_eq!(layouts(&s), ["Layout1", "Sheet A", "Sheet A (2)"]);
}
