use std::{collections::HashMap, io::Write, sync::Arc};

use slang::Downcast;
use wgpu::include_wgsl;
use wgpu_tidy::{reflection::{layout_entries_wowee, walk_him_down, Cursor}, AppState};
use winit::event_loop::{self, EventLoop};

fn just_validate_wgsl() {


    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
        backends: wgpu::Backends::VULKAN | wgpu::Backends::METAL,
        ..Default::default()
    });

    let adapter_future = instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        force_fallback_adapter: false,
        compatible_surface: None,
    });
    let adapter = pollster::block_on(adapter_future).unwrap();

    let device_future = adapter.request_device(
        &wgpu::DeviceDescriptor {
            label: Some("device"),
            required_features: wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES,
            required_limits: wgpu::Limits {
                max_bind_groups: 5,
                ..Default::default()
            },
            memory_hints: wgpu::MemoryHints::default(),
        },
        None,
    );
    let (device, queue) = pollster::block_on(device_future).unwrap();
    let shader_module = device.create_shader_module(include_wgsl!("shader/testy5.wgsl"));
}

fn main() {
    just_validate_wgsl();
    // let event_loop = EventLoop::new().unwrap();

    // event_loop.set_control_flow(event_loop::ControlFlow::Poll);

    // let mut app_state = AppState::Uninitialized();
    // event_loop.run_app(&mut app_state).unwrap();

    // let global_session = slang::GlobalSession::new().unwrap();
    // let search_path = std::ffi::CString::new("src/shader").unwrap();

    // let session_options = slang::CompilerOptions::default().matrix_layout_row(true);

    // let target_desc = slang::TargetDesc::default()
    //     .format(slang::CompileTarget::Wgsl)
    //     .profile(global_session.find_profile("glsl_450"));

    // let targets = [target_desc];
    // let search_paths = [search_path.as_ptr()];

    // let session_desc = slang::SessionDesc::default()
    //     .targets(&targets)
    //     .search_paths(&search_paths)
    //     .options(&session_options);

    // let session = global_session.create_session(&session_desc).unwrap();

    // let module = session.load_module("gorilla.slang").unwrap();

    // let frag_entry_point = module.find_entry_point_by_name("fragment").unwrap();

    // let program = session
    //     .create_composite_component_type(&[
    //         module.downcast().clone(),
    //         frag_entry_point.downcast().clone(),
    //     ])
    //     .unwrap();

    // let linked_program = program.link().unwrap();
    // let shader_bytecode = linked_program.target_code(0).unwrap();

	
    // let reflection = linked_program.layout(0).unwrap();
    // let global_type_layout = reflection.global_params_type_layout();
    // let global_var_layout = reflection.global_params_var_layout();
	
    // println!("Hello, world!");
    // walk_him_down(global_var_layout, Cursor::fresh(global_type_layout));
    // let mut entries = HashMap::new();
    // layout_entries_wowee(global_type_layout, &mut entries, 0, 0);
    // println!("{:?}", entries);

    // let mut file = std::fs::File::create("filename.wgsl").unwrap();
    // file.write_all(shader_bytecode.as_slice()).unwrap();
}
