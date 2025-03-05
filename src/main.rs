use std::{collections::HashMap, io::Write};

use slang::{Downcast};
use wgpu_tidy::{reflection::{print_var_layout, print_var_tree, var_tree, walk_increment_print, DescriptorTableSlotOffset, SimpleOffset, SubEROffset}, AppState};
use winit::event_loop::{self, EventLoop};

fn main() {
//     println!("Hello, world!");
//     let event_loop = EventLoop::new().unwrap();

//     event_loop.set_control_flow(event_loop::ControlFlow::Poll);

//     let mut app_state = AppState::Uninitialized();
//     event_loop.run_app(&mut app_state).unwrap();

    let global_session = slang::GlobalSession::new().unwrap();
	let search_path = std::ffi::CString::new("src/shader").unwrap();

	let session_options = slang::CompilerOptions::default().matrix_layout_row(true);

	let target_desc = slang::TargetDesc::default()
		.format(slang::CompileTarget::Wgsl)
		.profile(global_session.find_profile("glsl_450"));

	let targets = [target_desc];
	let search_paths = [search_path.as_ptr()];

	let session_desc = slang::SessionDesc::default()
		.targets(&targets)
		.search_paths(&search_paths)
		.options(&session_options);

	let session = global_session.create_session(&session_desc).unwrap();

	let module = session.load_module("gorilla.slang").unwrap();

	let frag_entry_point = module.find_entry_point_by_name("fragment").unwrap();

	let program = session
		.create_composite_component_type(&[
			module.downcast().clone(),
			frag_entry_point.downcast().clone(),
		])
		.unwrap();

	let linked_program = program.link().unwrap();
	let shader_bytecode = linked_program.target_code(0).unwrap();


    let reflection = linked_program.layout(0).unwrap();
    let global_type_layout = reflection.global_params_type_layout();
	let global_var_layout = reflection.global_params_var_layout();

	let mut entries = HashMap::new();
    wgpu_tidy::reflection::layout_entries_wowee(global_type_layout,&mut entries,0,0);

	println!("{:?}", entries);
	print_var_layout(global_var_layout);
	walk_increment_print(global_var_layout, wgpu_tidy::reflection::Offset::DescriptorTableSlotOffset(DescriptorTableSlotOffset::default()));

	
	let mut file = std::fs::File::create("filename.wgsl").unwrap();
	file.write_all(shader_bytecode.as_slice()).unwrap();
}
