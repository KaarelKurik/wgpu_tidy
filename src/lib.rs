#![feature(generic_const_exprs)]

pub mod reflection;

use std::{
    collections::HashMap,
    f32::consts::{PI, TAU},
    pin::Pin,
    sync::Arc,
};

use cgmath::{Basis3, Matrix3, Rad, Rotation, Rotation3, SquareMatrix, Vector3};
use encase::{
    ShaderType,
    internal::{BufferMut, WriteInto},
};
use reflection::{
    base_layout_entries, bind_group_entries_from_layout, buffers_from_layout, Cursor, Writable
};
use slang::Downcast;
use wgpu::*;
use winit::{
    application::ApplicationHandler,
    dpi::PhysicalSize,
    event::WindowEvent,
    event_loop::ActiveEventLoop,
    window::{Window, WindowId},
};

struct Gamma<T: ShaderType + WriteInto> {
    obj: T,
    set: u32,
    binding: u32,
}

trait GammaT {
    fn write_funny(&self, bgds: &[BindGroupDescriptor], q: Queue);
    fn binding_spot(&self) -> (u32, u32);
}

impl<T: ShaderType + WriteInto> GammaT for Gamma<T> {
    fn write_funny(&self, bgds: &[BindGroupDescriptor], q: Queue) {
        let bgd = &bgds[self.set as usize];
        let er = &bgd.entries[self.binding as usize].resource;
        match er {
            BindingResource::Buffer(buffer_binding) => {
                let mut ub = encase::UniformBuffer::new(Vec::<u8>::new());
                ub.write(&self.obj).unwrap();
                q.write_buffer(
                    buffer_binding.buffer,
                    buffer_binding.offset,
                    ub.as_ref().as_slice(),
                );
            }
            _ => unimplemented!("AAH"),
        }
    }
    fn binding_spot(&self) -> (u32, u32) {
        (self.set, self.binding)
    }
}

struct FunnyBusiness<'a> {
    buffers: HashMap<(u32, u32), Buffer>,
    bind_group_entries: HashMap<(u32, u32), BindGroupEntry<'a>>,
    x: HashMap<u32, BindGroupLayout>,
    y: HashMap<u32, Vec<BindGroupEntry<'a>>>,
    z: HashMap<u32, BindGroupDescriptor<'a>>,
}

struct WowBad {
    x: Pin<Box<f32>>,
    y: Option<&'static f32>,
}

fn make_wowbad() -> WowBad {
    let mut out = WowBad {
        x: Box::pin(0f32),
        y: None,
    };

    out.y = unsafe {
        let rr: &'static f32 = (&*out.x as *const f32).as_ref().unwrap();
        Some(rr)
    };
    out
}

struct GlobalShaderScope {}

impl GlobalShaderScope {
    fn bind_group_layout_entries() -> Vec<Vec<BindGroupLayoutEntry>> {
        let visibility = ShaderStages::all();
        vec![]
    }
}

struct Camera {
    width: f32,
    height: f32,
    frame: Matrix3<f32>,
    frame_inv: Matrix3<f32>,
    centre: Vector3<f32>,
    yfov: f32,
}

struct TR3 {
    q: Vector3<f32>,
    v: Vector3<f32>,
}

#[derive(Debug)]
struct Hermite {
    pos: Vector3<f32>,
    normal: Vector3<f32>,
}

struct SurfaceParams {
    support: f32,
    point_count: i32,
    point_data: Vec<Hermite>,
}

struct GraphicsGlobal {
    camera: Camera,
    surface: SurfaceParams,
}

trait ToBytes {
    const BYTE_COUNT: usize;

    fn to_bytes(&self) -> [u8; Self::BYTE_COUNT];
}
impl ToBytes for Vector3<f32> {
    const BYTE_COUNT: usize = 12;
    fn to_bytes(&self) -> [u8; Self::BYTE_COUNT] {
        let mut data: [u8; Self::BYTE_COUNT] = [0; Self::BYTE_COUNT];
        data.write(0 * 4, &self.x.to_le_bytes());
        data.write(1 * 4, &self.y.to_le_bytes());
        data.write(2 * 4, &self.z.to_le_bytes());
        data
    }
}

impl ToBytes for Matrix3<f32> {
    const BYTE_COUNT: usize = 3 * 4 * 4;

    // Row-major
    fn to_bytes(&self) -> [u8; Self::BYTE_COUNT] {
        let mut data: [u8; Self::BYTE_COUNT] = [0; Self::BYTE_COUNT];
        let c1 = self.x.to_bytes();
        let c2 = self.y.to_bytes();
        let c3 = self.z.to_bytes();
        data.write_slice(0, &c1[0..4]);
        data.write_slice(4, &c2[0..4]);
        data.write_slice(8, &c3[0..4]);
        data.write_slice(16, &c1[4..8]);
        data.write_slice(20, &c2[4..8]);
        data.write_slice(24, &c3[4..8]);
        data.write_slice(32, &c1[8..12]);
        data.write_slice(36, &c2[8..12]);
        data.write_slice(40, &c3[8..12]);
        data
    }
}

impl Writable for f32 {
    fn write_at_cursor(
        &self,
        c: reflection::Cursor,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        buffers: &mut HashMap<usize, HashMap<usize, Buffer>>,
    ) {
        let bind_group_ix = c.offset().set();
        let bind_slot_ix = c.offset().slot();
        let uniform_offset = c.offset().uniform();
        let buffer = buffers
            .get(&bind_group_ix)
            .unwrap()
            .get(&bind_slot_ix)
            .unwrap();
        queue.write_buffer(
            buffer,
            uniform_offset.try_into().unwrap(),
            &self.to_le_bytes(),
        );
    }
}

impl Writable for i32 {
    fn write_at_cursor(
        &self,
        c: reflection::Cursor,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        buffers: &mut HashMap<usize, HashMap<usize, Buffer>>,
    ) {
        let bind_group_ix = c.offset().set();
        let bind_slot_ix = c.offset().slot();
        let uniform_offset = c.offset().uniform();
        let buffer = buffers
            .get(&bind_group_ix)
            .unwrap()
            .get(&bind_slot_ix)
            .unwrap();
        queue.write_buffer(
            buffer,
            uniform_offset.try_into().unwrap(),
            &self.to_le_bytes(),
        );
    }
}

impl Writable for Vector3<f32> {
    // Technically an indexing operation, so can panic on wrong offsets
    fn write_at_cursor(
        &self,
        c: reflection::Cursor,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        buffers: &mut HashMap<usize, HashMap<usize, Buffer>>,
    ) {
        let bind_group_ix = c.offset().set();
        let bind_slot_ix = c.offset().slot();
        let uniform_offset = c.offset().uniform();
        let buffer = buffers
            .get(&bind_group_ix)
            .unwrap()
            .get(&bind_slot_ix)
            .unwrap();
        let data = self.to_bytes();
        queue.write_buffer(buffer, uniform_offset.try_into().unwrap(), &data);
    }
}

impl Writable for Matrix3<f32> {
    fn write_at_cursor(
        &self,
        c: reflection::Cursor,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        buffers: &mut HashMap<usize, HashMap<usize, Buffer>>,
    ) {
        let bind_group_ix = c.offset().set();
        let bind_slot_ix = c.offset().slot();
        let uniform_offset = c.offset().uniform();
        let buffer = buffers
            .get(&bind_group_ix)
            .unwrap()
            .get(&bind_slot_ix)
            .unwrap();
        let data = self.to_bytes();
        queue.write_buffer(buffer, uniform_offset.try_into().unwrap(), &data);
    }
}

impl Writable for Hermite {
    fn write_at_cursor(
        &self,
        c: reflection::Cursor,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        buffers: &mut HashMap<usize, HashMap<usize, Buffer>>,
    ) {
        let pos_cursor = c.navigate_field(0).unwrap();
        let normal_cursor = c.navigate_field(1).unwrap();
        self.pos.write_at_cursor(pos_cursor, device, queue, buffers);
        self.normal
            .write_at_cursor(normal_cursor, device, queue, buffers);
    }
}

impl Writable for Camera {
    fn write_at_cursor(
        &self,
        c: reflection::Cursor,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        buffers: &mut HashMap<usize, HashMap<usize, Buffer>>,
    ) {
        let width_cursor = c.navigate_field(0).unwrap();
        let height_cursor = c.navigate_field(1).unwrap();
        let frame_cursor = c.navigate_field(2).unwrap();
        let frame_inv_cursor = c.navigate_field(3).unwrap();
        let centre_cursor = c.navigate_field(4).unwrap();
        let yfov_cursor = c.navigate_field(5).unwrap();
        self.width
            .write_at_cursor(width_cursor, device, queue, buffers);
        self.height
            .write_at_cursor(height_cursor, device, queue, buffers);
        self.frame
            .write_at_cursor(frame_cursor, device, queue, buffers);
        self.frame_inv
            .write_at_cursor(frame_inv_cursor, device, queue, buffers);
        self.centre
            .write_at_cursor(centre_cursor, device, queue, buffers);
        self.yfov
            .write_at_cursor(yfov_cursor, device, queue, buffers);
    }
}

impl Writable for SurfaceParams {
    fn write_at_cursor(
        &self,
        c: reflection::Cursor,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        buffers: &mut HashMap<usize, HashMap<usize, Buffer>>,
    ) {
        let support_cursor = c.navigate_field(0).unwrap();
        let point_count_cursor = c.navigate_field(1).unwrap();
        let point_data_cursor = c.navigate_field(2).unwrap();
        self.support
            .write_at_cursor(support_cursor, device, queue, buffers);
        self.point_count
            .write_at_cursor(point_count_cursor, device, queue, buffers);
        // let total_bytes_needed = (self.point_count as usize)
        //     * point_data_cursor
        //         .type_layout()
        //         .element_stride(slang::ParameterCategory::Uniform);
        let total_bytes_needed = (self.point_count as usize)
            * point_data_cursor
                .type_layout()
                .element_type_layout()
                .stride(slang::ParameterCategory::Uniform);
        let point_data_buffer = buffers
            .get_mut(&point_data_cursor.offset().set())
            .unwrap()
            .get_mut(&point_data_cursor.offset().slot())
            .unwrap();
        if point_data_buffer.size() != total_bytes_needed.try_into().unwrap() {
            *point_data_buffer = device.create_buffer(&BufferDescriptor {
                label: None,
                size: total_bytes_needed.try_into().unwrap(),
                usage: point_data_buffer.usage(),
                mapped_at_creation: false,
            });
        }
        for i in 0..self.point_count {
            let hermite_cursor = point_data_cursor.navigate_index(i as u32).unwrap();
            self.point_data[i as usize].write_at_cursor(hermite_cursor, device, queue, buffers);
        }
    }
}

impl Writable for GraphicsGlobal {
    fn write_at_cursor(
        &self,
        c: reflection::Cursor,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        buffers: &mut HashMap<usize, HashMap<usize, Buffer>>,
    ) {
        let camera_cursor = c.navigate_field(0).unwrap().navigate_child().unwrap();
        let surface_cursor = c.navigate_field(1).unwrap().navigate_child().unwrap();
        self.camera
            .write_at_cursor(camera_cursor, device, queue, buffers);
        self.surface
            .write_at_cursor(surface_cursor, device, queue, buffers);
    }
}

pub enum AppState<'a> {
    Uninitialized(),
    Initialized(App<'a>),
}

pub struct App<'a> {
    window: Arc<Window>,
    size: PhysicalSize<u32>,
    surface: Surface<'a>,
    surface_config: wgpu::SurfaceConfiguration,
    device: Device,
    queue: Queue,
    render_pipeline: RenderPipeline,
    buffers: HashMap<usize, HashMap<usize, Buffer>>,
    bind_group_layouts: HashMap<usize, BindGroupLayout>,
    layout_entries: HashMap<usize, Vec<BindGroupLayoutEntry>>,
    graphics_global: GraphicsGlobal,
    program: slang::ComponentType,
    slang_global_session: slang::GlobalSession,
    slang_session: slang::Session,
}

fn circle(n: usize) -> Vec<Hermite> {
    (0..n)
        .map(|i| {
            let rot = <Basis3<_> as Rotation3>::from_angle_z(Rad(TAU * (i as f32 / n as f32)));
            let v = rot.rotate_vector(Vector3::unit_x());
            Hermite { pos: v, normal: v }
        })
        .collect()
}

impl<'a> App<'a> {
    fn render(&mut self) -> Result<(), SurfaceError> {
        // Global graphics object?
        let linked_program = self.program.link().unwrap();
        let reflection = linked_program.layout(0).unwrap();
        let global_type_layout = reflection.global_params_type_layout();

        let top_cursor = Cursor::fresh(global_type_layout);
        self.graphics_global.write_at_cursor(top_cursor, &self.device, &self.queue, &mut self.buffers);

        // Make bind groups
        let bind_group_entries =
            bind_group_entries_from_layout(&self.layout_entries, &self.buffers);
        let bind_group_labels: HashMap<usize, String> = bind_group_entries
            .iter()
            .map(|(&k, v)| (k, format!("bg{}", k)))
            .collect();
        let bind_group_descriptors = bind_group_entries.iter().map(|(&k, _v)| {
            (
                k,
                BindGroupDescriptor {
                    label: Some(bind_group_labels.get(&k).unwrap()),
                    layout: self.bind_group_layouts.get(&k).unwrap(),
                    entries: bind_group_entries.get(&k).unwrap(),
                },
            )
        });
        let bind_groups: HashMap<usize, BindGroup> = bind_group_descriptors
            .map(|(k, d)| (k, self.device.create_bind_group(&d)))
            .collect();
        // Render stuff
        let output = self.surface.get_current_texture()?;
        let view = output
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("encoder"),
            });
        {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("render_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.1,
                            g: 0.2,
                            b: 0.3,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            render_pass.set_pipeline(&self.render_pipeline);
            for (k, bg) in bind_groups {
                render_pass.set_bind_group(k.try_into().unwrap(), &bg, &[]);
            }
            render_pass.draw(0..3, 0..1);
        }
        self.queue.submit(std::iter::once(encoder.finish()));
        output.present();
        Ok(())
    }
    fn resize(&mut self, new_size: winit::dpi::PhysicalSize<u32>) {
        if new_size.width > 0 && new_size.height > 0 {
            self.size = new_size;
            self.surface_config.width = new_size.width;
            self.surface_config.height = new_size.height;
            self.surface.configure(&self.device, &self.surface_config);
        }
    }
}

impl<'a> ApplicationHandler for AppState<'a> {
    fn new_events(
        &mut self,
        event_loop: &winit::event_loop::ActiveEventLoop,
        cause: winit::event::StartCause,
    ) {
        match self {
            AppState::Uninitialized() => {}
            AppState::Initialized(app) => {
                if cause == winit::event::StartCause::Poll {
                    app.window.request_redraw();
                }
            }
        }
    }
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if let AppState::Initialized(_) = self {
            panic!("Tried to initialize already-initialized app!");
        }

        let window = Arc::new(
            event_loop
                .create_window(Window::default_attributes())
                .unwrap(),
        );

        let size = window.as_ref().inner_size();

        let instance = wgpu::Instance::new(&InstanceDescriptor {
            backends: wgpu::Backends::VULKAN | wgpu::Backends::METAL,
            ..Default::default()
        });

        let surface = instance.create_surface(window.clone()).unwrap();

        let adapter_future = instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            force_fallback_adapter: false,
            compatible_surface: Some(&surface),
        });
        let adapter = pollster::block_on(adapter_future).unwrap();

        let surface_caps = surface.get_capabilities(&adapter);
        let supported_presentation_modes = surface_caps.present_modes;

        let mode_comparator = |pres_mode: &&wgpu::PresentMode| match pres_mode {
            wgpu::PresentMode::Immediate => -1, // my machine freezes every few secs with vsync now - not sure why
            wgpu::PresentMode::Mailbox => 0,
            wgpu::PresentMode::FifoRelaxed => 1,
            wgpu::PresentMode::Fifo => 2,
            _ => 3,
        };
        let present_mode = *supported_presentation_modes
            .iter()
            .min_by_key(mode_comparator)
            .unwrap();

        let surface_format = surface_caps
            .formats
            .iter()
            .find(|f| f.is_srgb())
            .copied()
            .unwrap_or(surface_caps.formats[0]);

        let surface_config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: surface_format,
            width: size.width,
            height: size.height,
            present_mode,
            desired_maximum_frame_latency: 2,
            alpha_mode: surface_caps.alpha_modes[0],
            view_formats: vec![],
        };

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

        surface.configure(&device, &surface_config); // causes segfault if device, surface_config die.

        // Slang block
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
        let vertex_entry_point = module.find_entry_point_by_name("vertex").unwrap();

        let program = session
            .create_composite_component_type(&[
                module.downcast().clone(),
                frag_entry_point.downcast().clone(),
                vertex_entry_point.downcast().clone(),
            ])
            .unwrap();

        let linked_program = program.link().unwrap();
        let shader_bytecode = linked_program.target_code(0).unwrap();

        let reflection = linked_program.layout(0).unwrap();
        let global_type_layout = reflection.global_params_type_layout();

        let layout_entries = base_layout_entries(global_type_layout);
        let buffers = buffers_from_layout(&device, &layout_entries);

        println!("{}", shader_bytecode.as_str().unwrap());

        // Using Slang-compiled code
        let shader_module = device.create_shader_module(ShaderModuleDescriptor {
            label: None,
            source: ShaderSource::Wgsl(std::borrow::Cow::Borrowed(
                shader_bytecode.as_str().unwrap(),
            )),
        });

        let bind_group_layout_descriptors: HashMap<usize, BindGroupLayoutDescriptor> =
            layout_entries
                .iter()
                .map(|(&k, v)| {
                    (
                        k,
                        BindGroupLayoutDescriptor {
                            label: None,
                            entries: v,
                        },
                    )
                })
                .collect();

        let bind_group_layouts: HashMap<usize, BindGroupLayout> = bind_group_layout_descriptors
            .iter()
            .map(|(&k, d)| (k, device.create_bind_group_layout(d)))
            .collect();

        let bind_group_layouts_vec: Vec<&BindGroupLayout> =
            bind_group_layouts.iter().map(|(_, v)| v).collect();
        // let bge = BindGroupEntry {
        //     binding: 0,
        //     resource: BindingResource::Buffer(BufferBinding {
        //         buffer: (),
        //         offset: (),
        //         size: (),
        //     }),
        // };

        let render_pipeline_layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
            label: Some("render_pipeline_layout"),
            bind_group_layouts: &bind_group_layouts_vec,
            push_constant_ranges: &[],
        });

        let render_pipeline = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some("render_pipeline"),
            layout: Some(&render_pipeline_layout),
            vertex: VertexState {
                module: &shader_module,
                entry_point: Some("vertex"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: PrimitiveState {
                topology: PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: FrontFace::Ccw,
                cull_mode: Some(Face::Back),
                unclipped_depth: false,
                polygon_mode: PolygonMode::Fill,
                conservative: false,
            },
            depth_stencil: None,
            multisample: MultisampleState {
                count: 1,
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            fragment: Some(FragmentState {
                module: &shader_module,
                entry_point: Some("fragment"),
                compilation_options: PipelineCompilationOptions::default(),
                targets: &[Some(ColorTargetState {
                    format: surface_format,
                    blend: None,
                    write_mask: ColorWrites::ALL,
                })],
            }),
            multiview: None,
            cache: None,
        });

        let camera = Camera {
            width: size.width as f32,
            height: size.height as f32,
            frame: Matrix3::identity(),
            frame_inv: Matrix3::identity(),
            centre: Vector3::new(0.0, 0.0, -5.0),
            yfov: PI / 4.0,
        };

        let point_data= circle(3);
        let surface_params = SurfaceParams {
            support: 1.0,
            point_count: point_data.len().try_into().unwrap(),
            point_data,
        };

        let graphics_global = GraphicsGlobal {
            camera,
            surface: surface_params,
        };

        *self = AppState::Initialized(App {
            window,
            surface,
            size,
            surface_config,
            device,
            queue,
            render_pipeline,
            bind_group_layouts,
            layout_entries,
            buffers,
            graphics_global,
            program,
            slang_global_session: global_session,
            slang_session: session,
        })
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        match self {
            AppState::Uninitialized() => {}
            AppState::Initialized(app) => match event {
                WindowEvent::CloseRequested => {
                    println!("The close button was pressed; stopping");
                    event_loop.exit();
                }
                WindowEvent::RedrawRequested => match app.render() {
                    Ok(_) => {}
                    Err(SurfaceError::Lost) => app.resize(app.size),
                    Err(SurfaceError::OutOfMemory) => event_loop.exit(),
                    Err(e) => eprintln!("{:?}", e),
                },
                WindowEvent::Resized(new_size) => app.resize(new_size),
                _ => (),
            },
        }
    }
}
