//! The wgpu device both apps create asks for the adapter's largest buffer (#2291): BEDIT's Edit
//! Block Definition preview of a whole architectural plan needed a 354 MB vertex buffer, over
//! wgpu's default 256 MiB cap, and the web canvas stayed black. Creates a real device through
//! the same setup hook the desktop and web apps use.

use cadcraft_ui_egui::gpu;

#[test]
fn the_device_gets_the_adapters_max_buffer_size() {
    let mut setup = egui_kittest::wgpu::default_wgpu_setup();
    gpu::request_adapter_buffer_limit(&mut setup);
    let rs = egui_kittest::wgpu::create_render_state(setup, Default::default());
    let adapter = rs.adapter.limits().max_buffer_size;
    assert_eq!(rs.device.limits().max_buffer_size, adapter, "the device is capped below what the adapter supports");
}
