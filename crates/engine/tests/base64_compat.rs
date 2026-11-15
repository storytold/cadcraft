//! File and automation inputs retain CADCraft's accepted Base64 forms.
use cadcraft_engine::cmd::file::{base64_decode, base64_encode};

#[test]
fn legacy_inputs_and_standard_outputs_survive_the_shared_cutover() {
    for bytes in [&b""[..], b"f", b"fo", b"foo", b"foobar", &[0, 255, 127]] {
        assert_eq!(base64_decode(&base64_encode(bytes)).as_deref(), Some(bytes));
    }
    assert_eq!(base64_decode("+_8=").as_deref(), Some(&[251, 255][..]));
    assert_eq!(base64_decode("Z=m 9\nv\r").as_deref(), Some(&b"foo"[..]));
    assert_eq!(base64_decode("A").as_deref(), Some(&b""[..]));
    assert!(base64_decode("Zm9v\t").is_none());
    assert!(base64_decode("Zm9v!").is_none());
}
