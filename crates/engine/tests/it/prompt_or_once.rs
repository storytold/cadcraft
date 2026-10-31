//! Prompts that offer options say "or" once, as AutoCAD 2024's do: "Enter angle of xline (0) or
//! [Reference]:". The prompt adds "or" before its options, so a message that ended in "or" read
//! "or or".

use cadcraft_engine::Session;
use serde_json::json;

fn shows(s: &Session, text: &str) {
    let p = s.prompt_text();
    assert!(p.ends_with(text), "{p}");
    assert!(!p.contains(" or or "), "{p}");
}

#[test]
fn xline_angle_and_offset() {
    let mut s = Session::new();
    s.cmdline("xline").unwrap();
    s.cmdline("a").unwrap();
    shows(&s, "Enter angle of xline (0) or [Reference]:");
    s.cancel();
    s.cmdline("xline").unwrap();
    s.cmdline("o").unwrap();
    shows(&s, "Specify offset distance or [Through] <1.0000>:");
}

#[test]
fn helix_height() {
    let mut s = Session::new();
    s.script("helix\n0,0\n5\n\n").unwrap();
    shows(&s, "Specify helix height or [Axis endpoint/Turns/turn Height/tWist] <1.0000>:");
}

#[test]
fn blend_first_object() {
    let mut s = Session::new();
    s.cmdline("blend").unwrap();
    shows(&s, "Select first object or [CONtinuity]:");
}

#[test]
fn lengthen_options() {
    let mut s = Session::new();
    s.cmdline("lengthen").unwrap();
    shows(&s, "Select an object to measure or [DElta/Percent/Total/DYnamic] <DYnamic>:");
    s.cmdline("de").unwrap();
    shows(&s, "Enter delta length or [Angle] <0.0000>:");
    s.cmdline("2").unwrap();
    shows(&s, "Select an object to change or [Undo]:");
    s.cancel();
    s.cmdline("lengthen").unwrap();
    s.cmdline("t").unwrap();
    shows(&s, "Specify total length or [Angle] <1.0000>:");
}

#[test]
fn ncopy_pedit_and_table_break() {
    let mut s = Session::new();
    s.script("line 25,-5 25,5\n\nline 0,0 10,0\n\n-block B 0,0 L\n\n").unwrap();
    s.execute("insert", &json!({ "name": "B", "at": [20, 0] })).unwrap();
    s.cmdline("zoom e").unwrap();
    s.cmdline("ncopy").unwrap();
    shows(&s, "Select nested objects to copy or [Settings]:");
    s.script("22,0\n\n").unwrap();
    shows(&s, "Specify base point or [Displacement/Multiple] <Displacement>:");
    s.cancel();
    s.cmdline("pedit").unwrap();
    shows(&s, "Select polyline or [Multiple]:");
    s.cancel();
    s.execute("table", &json!({ "at": [0, 20], "cells": [["1", ""], ["2", ""]], "rowHeight": 1.0, "colWidth": 3.0 })).unwrap();
    s.cmdline("table.break").unwrap();
    s.cmdline("1,19.5").unwrap();
    shows(&s, "Enter maximum part height or [Off]:");
}
