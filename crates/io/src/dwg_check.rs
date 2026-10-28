//! What CADCraft's own DWG reader (`cadcraft_dwg::codec`) finds in a file, used to correct what the
//! DWG bridge (acadrust, through DXF) hands over.

use cadcraft_doc::{Drawing, Handle, KeptHandle};

/// The space blocks' end markers keep the file's handles. The bridge gives the paper space block's
/// ENDBLK a new handle; the file records which BLOCK and ENDBLK sit in each space, so a layout whose
/// begin marker is the file's and whose end marker is a handle the file doesn't have gets the
/// file's end marker back (when nothing else in the drawing has that handle).
pub(crate) fn space_block_ends(d: &mut Drawing, bytes: &[u8]) {
    let Ok(c) = cadcraft_dwg::codec::read_container(bytes) else { return };
    let m = c.space_markers();
    let free = |d: &Drawing, h: u64| d.entity(Handle(h)).is_none() && !d.layouts.iter().any(|l| l.end_handle.get() == Handle(h));
    let fix = |begin: KeptHandle, end: KeptHandle, file_begin: Option<u64>, file_end: Option<u64>| -> Option<KeptHandle> {
        let (b, e) = (file_begin?, file_end?);
        (begin.get() == Handle(b) && end.get() != Handle(e) && !c.has_handle(end.get().0)).then_some(KeptHandle(Handle(e)))
    };
    for i in 0..d.layouts.len() {
        let Some(l) = d.layouts.get(i) else { continue };
        if let Some(new) = fix(l.begin_handle, l.end_handle, m.paper_begin, m.paper_end)
            && free(d, new.get().0)
            && let Some(l) = d.layouts.get_mut(i)
        {
            l.end_handle = new;
        }
    }
    if let Some(l) = d.model_layout.as_deref()
        && let Some(new) = fix(l.begin_handle, l.end_handle, m.model_begin, m.model_end)
        && free(d, new.get().0)
        && let Some(l) = d.model_layout.as_deref_mut()
    {
        l.end_handle = new;
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
#[allow(clippy::unwrap_used)] // the helper below is test code too
mod tests {
    use super::*;
    use cadcraft_doc::geom::Vec3;
    use cadcraft_doc::{EntityKind, Line, Space};

    /// A DWG CADCraft wrote, with the paper space block's markers at 1B and 1D, and the drawing
    /// read from it.
    fn saved() -> (Vec<u8>, Drawing) {
        let mut d = Drawing::new_imperial();
        if let Some(l) = d.layouts.first_mut() {
            (l.begin_handle, l.end_handle) = (KeptHandle(Handle(0x1B)), KeptHandle(Handle(0x1D)));
        }
        d.add(&Space::Model, Default::default(), EntityKind::Line(Line { a: Vec3::new(0.0, 0.0, 0.0), b: Vec3::new(1.0, 0.0, 0.0) })).unwrap();
        let bytes = crate::write(&d, "x.dwg").unwrap();
        let read = crate::read(&bytes, "x.dwg").unwrap();
        (bytes, read)
    }

    /// The bridge's symptom (the paper space ENDBLK with a handle the file doesn't have) is put
    /// right from the file.
    #[test]
    fn a_renumbered_paper_space_end_marker_gets_the_files_handle() {
        let (bytes, mut d) = saved();
        let l = d.layouts.iter_mut().find(|l| l.begin_handle.get() == Handle(0x1B)).unwrap();
        l.end_handle = KeptHandle(Handle(0x3D13A));
        space_block_ends(&mut d, &bytes);
        let l = d.layouts.iter().find(|l| l.begin_handle.get() == Handle(0x1B)).unwrap();
        assert_eq!(l.end_handle.get(), Handle(0x1D));
    }

    /// Nothing changes when the drawing already has the file's markers, when its end marker is a
    /// handle the file has (another object's: not the bridge's symptom), when the layout isn't the
    /// file's paper space block, or when the bytes aren't a DWG CADCraft's reader takes apart.
    #[test]
    fn markers_that_are_right_or_not_the_symptom_are_left_alone() {
        let (bytes, d) = saved();
        let mut same = d.clone();
        space_block_ends(&mut same, &bytes);
        assert_eq!(
            same.layouts.iter().map(|l| l.end_handle.get()).collect::<Vec<_>>(),
            d.layouts.iter().map(|l| l.end_handle.get()).collect::<Vec<_>>()
        );
        let model_line = d.model.iter().next().unwrap().handle;
        for (begin, end) in [(Handle(0x1B), model_line), (Handle(0x99), Handle(0x3D13A))] {
            let mut x = d.clone();
            let l = x.layouts.iter_mut().find(|l| l.begin_handle.get() == Handle(0x1B)).unwrap();
            (l.begin_handle, l.end_handle) = (KeptHandle(begin), KeptHandle(end));
            space_block_ends(&mut x, &bytes);
            let l = x.layouts.iter().find(|l| l.begin_handle.get() == begin).unwrap();
            assert_eq!(l.end_handle.get(), end, "begin {begin:?}");
        }
        let mut x = d.clone();
        let l = x.layouts.iter_mut().find(|l| l.begin_handle.get() == Handle(0x1B)).unwrap();
        l.end_handle = KeptHandle(Handle(0x3D13A));
        space_block_ends(&mut x, b"not a DWG at all");
        assert!(x.layouts.iter().any(|l| l.end_handle.get() == Handle(0x3D13A)));
    }
}
