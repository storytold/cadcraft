//! `setvar CECOLOR` takes what the `color` command takes: "ByLayer", "ByBlock", a colour name or an
//! index (#1999). `getvar CECOLOR` reads it back as AutoCAD does, as text: "BYLAYER", "BYBLOCK",
//! the index ("1" for red) or "RGB:r,g,b".

use cadcraft_engine::Session;
use cadcraft_engine::doc::color::{Color, Rgb};
use serde_json::{Value, json};

fn cecolor(s: &mut Session) -> Value {
    s.execute("getvar", &json!({ "name": "CECOLOR" })).unwrap()["value"].clone()
}

fn new_line_color(s: &mut Session) -> Color {
    s.execute("line", &json!({ "points": [[0, 0], [10, 0]] })).unwrap();
    s.doc().unwrap().model.last().unwrap().common.color
}

#[test]
fn getvar_cecolor_of_a_new_drawing_is_bylayer() {
    let mut s = Session::new();
    assert_eq!(cecolor(&mut s), json!("BYLAYER"));
}

#[test]
fn setvar_cecolor_accepts_names() {
    for (value, shown) in [
        ("BYLAYER", "BYLAYER"),
        ("ByLayer", "BYLAYER"),
        ("256", "BYLAYER"),
        ("BYBLOCK", "BYBLOCK"),
        ("0", "BYBLOCK"),
        ("red", "1"),
        ("7", "7"),
        ("250", "250"),
        ("RGB:10,20,30", "RGB:10,20,30"),
        ("10,20,30", "RGB:10,20,30"),
    ] {
        let mut s = Session::new();
        s.execute("setvar", &json!({ "name": "CECOLOR", "value": value })).unwrap_or_else(|e| panic!("{value}: {e}"));
        assert_eq!(cecolor(&mut s), json!(shown), "{value}");
    }
}

#[test]
fn getvar_cecolor_after_the_color_command() {
    let mut s = Session::new();
    s.execute("color", &json!({ "color": "ByBlock" })).unwrap();
    assert_eq!(cecolor(&mut s), json!("BYBLOCK"));
    s.execute("color", &json!({ "color": "10,20,30" })).unwrap();
    assert_eq!(cecolor(&mut s), json!("RGB:10,20,30"));
}

/// The value `getvar` gives back is one `setvar` takes, so saving and restoring keeps a true colour.
#[test]
fn getvar_cecolor_restores_a_true_colour() {
    let mut s = Session::new();
    s.execute("color", &json!({ "color": "10,20,30" })).unwrap();
    let saved = cecolor(&mut s);
    s.execute("setvar", &json!({ "name": "CECOLOR", "value": "250" })).unwrap();
    s.execute("setvar", &json!({ "name": "CECOLOR", "value": saved })).unwrap();
    assert_eq!(new_line_color(&mut s), Color::True(Rgb(10, 20, 30)));
}

#[test]
fn setvar_cecolor_lists_cecolor_as_text() {
    let mut s = Session::new();
    s.execute("setvar", &json!({ "name": "CECOLOR", "value": "red" })).unwrap();
    assert_eq!(cadcraft_engine::sysvars::list(&s)["CECOLOR"], json!("1"));
}

#[test]
fn setvar_cecolor_name_replaces_a_true_colour() {
    let mut s = Session::new();
    s.execute("color", &json!({ "color": "10,20,30" })).unwrap();
    assert_eq!(new_line_color(&mut s), Color::True(Rgb(10, 20, 30)));
    s.execute("setvar", &json!({ "name": "CECOLOR", "value": "ByLayer" })).unwrap();
    assert_eq!(cecolor(&mut s), json!("BYLAYER"));
    assert_eq!(new_line_color(&mut s), Color::ByLayer);
}

#[test]
fn setvar_cecolor_number_replaces_a_true_colour() {
    let mut s = Session::new();
    s.execute("color", &json!({ "color": "10,20,30" })).unwrap();
    s.execute("setvar", &json!({ "name": "CECOLOR", "value": 250 })).unwrap();
    assert_eq!(new_line_color(&mut s), Color::Index(250));
}

#[test]
fn setvar_cecolor_refuses_what_is_not_a_colour() {
    let mut s = Session::new();
    for bad in [json!("chartreuse"), json!(257), json!(-1), json!(true), json!("RGB:"), json!("RGB:300,0,0")] {
        assert!(s.execute("setvar", &json!({ "name": "CECOLOR", "value": bad })).is_err(), "{bad}");
    }
    assert_eq!(cecolor(&mut s), json!("BYLAYER"));
}
