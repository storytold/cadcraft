//! Drawing output is called Plot, as in standard CAD convention; PRINT stays an alias (issue #443).

use cadcraft_engine::{cmd::resolve_alias, command_specs, find_command};

#[test]
fn plot_is_the_command_and_print_is_its_alias() {
    let plot = find_command("plot").unwrap();
    assert_eq!(plot.label, "Plot...");
    assert_eq!(plot.menu, ["File", "Plot..."]);
    assert_eq!(plot.shortcut, Some("Cmd+P"));
    assert!(plot.aliases.contains(&"print"));
    assert_eq!(resolve_alias("print"), "plot");
    assert_eq!(resolve_alias("PRINT"), "plot");
    assert_eq!(resolve_alias("plot"), "plot");
}

#[test]
fn no_command_label_or_menu_says_print() {
    for c in command_specs() {
        let says_print = |s: &str| s.split(|ch: char| !ch.is_ascii_alphabetic()).any(|w| w.eq_ignore_ascii_case("print"));
        assert!(!says_print(c.label), "{}: label {:?}", c.id, c.label);
        assert!(!c.menu.iter().any(|m| says_print(m)), "{}: menu {:?}", c.id, c.menu);
    }
}
