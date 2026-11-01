use cadcraft_engine::units::parse_angle;

#[test]
fn bearing_rejects_out_of_range_angles() {
    for s in ["n100e", "n-45e", "n91e", "s-1w", "n-0.5e"] {
        assert_eq!(parse_angle(s), None, "{s}");
    }
    for s in ["n0e", "n90e", "s45w"] {
        assert!(parse_angle(s).is_some(), "{s}");
    }
}
