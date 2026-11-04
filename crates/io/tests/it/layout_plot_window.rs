//! #1667: a layout whose page setup plots a window (DXF plot type 4) lost the window corners
//! (groups 48, 49, 140, 141) on save: they were written as zero.

use cadcraft_doc::{Drawing, Layout, PageSetup};
use cadcraft_io::{read, write_dxf};

#[test]
fn plot_window_corners_round_trip_through_dxf() {
    let mut d = Drawing::new_metric();
    let mut l = Layout::new("WIN", 3);
    l.page = PageSetup {
        width_mm: 297.0,
        height_mm: 420.0,
        margins_mm: [0.0; 4],
        plot_area: "window".into(),
        plot_window: [0.0, -297.0, 420.0, 0.0],
        ..PageSetup::default()
    };
    d.layouts.push(l);

    let back = read(write_dxf(&d).as_bytes(), "win.dxf").unwrap();
    let page = &back.layouts.iter().find(|l| l.name == "WIN").unwrap().page;
    assert_eq!(page.plot_area, "window");
    assert_eq!(page.plot_window, [0.0, -297.0, 420.0, 0.0]);
}
