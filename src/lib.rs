#![feature(generic_const_exprs)]

pub mod reflection;

use std::{
    collections::HashMap,
    f32::consts::{PI, TAU},
    path::Path,
    pin::Pin,
    sync::Arc,
    time::{Duration, Instant},
};

use cgmath::{
    Array, Basis3, InnerSpace, Matrix3, Rad, Rotation, Rotation3, SquareMatrix, Vector3, Zero,
};
use encase::{
    ShaderType,
    internal::{BufferMut, WriteInto},
};
use image::{EncodableLayout, ImageError, RgbaImage};
use proc_macros::Writable;
use reflection::{
    BindingResources, Cursor, Writable, base_layout_entries, bind_group_entries_from_layout,
    buffers_from_layout,
};
use slang::Downcast;
use wgpu::{*};
use winit::{
    application::ApplicationHandler,
    dpi::PhysicalSize,
    event::{DeviceEvent, ElementState, KeyEvent, MouseButton, WindowEvent},
    event_loop::ActiveEventLoop,
    keyboard::{KeyCode, PhysicalKey},
    window::{CursorGrabMode, Window, WindowId},
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

// Has to have equal resolution on all faces
struct RgbaSkybox {
    px: RgbaImage,
    nx: RgbaImage,
    py: RgbaImage,
    ny: RgbaImage,
    pz: RgbaImage,
    nz: RgbaImage,
}

impl RgbaSkybox {
    fn dimensions(&self) -> (u32, u32) {
        self.px.dimensions()
    }
    fn width(&self) -> u32 {
        self.px.width()
    }
    fn height(&self) -> u32 {
        self.px.height()
    }
    fn extent(&self) -> Extent3d {
        Extent3d {
            width: self.width(),
            height: self.height(),
            depth_or_array_layers: 6,
        }
    }
    fn texture_format(&self) -> TextureFormat {
        TextureFormat::Rgba8UnormSrgb
    }
    fn descriptor(&self) -> TextureDescriptor {
        TextureDescriptor {
            label: None,
            size: self.extent(),
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: self.texture_format(),
            usage: TextureUsages::COPY_DST | TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        }
    }
    fn view_descriptor(&self) -> TextureViewDescriptor {
        TextureViewDescriptor {
            dimension: Some(TextureViewDimension::Cube),
            ..Default::default()
        }
    }
    fn as_bytevec(&self) -> Vec<u8> {
        [
            self.px.as_bytes(),
            self.nx.as_bytes(),
            self.py.as_bytes(),
            self.ny.as_bytes(),
            self.pz.as_bytes(),
            self.nz.as_bytes(),
        ]
        .concat()
    }
    fn bytes_per_block(&self) -> u32 {
        4
    }
    fn load_from_path(bg_path: &Path) -> Result<Self, ImageError> {
        let [px, nx, py, ny, pz, nz]: [Result<_, ImageError>; 6] =
            ["right", "left", "bottom", "top", "front", "back"]
                .map(|x| {
                    let mut im = image::open(bg_path.join(format!("{}.png", x)))?.into_rgba8();
                    image::imageops::flip_vertical_in_place(&mut im);
                    Ok(im)});
        Ok(RgbaSkybox {
            px: px?,
            nx: nx?,
            py: py?,
            ny: ny?,
            pz: pz?,
            nz: nz?,
        })
    }
}

impl Writable for RgbaSkybox {
    fn write_at_cursor(
        &self,
        c: Cursor,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        binding_resources: &mut BindingResources,
    ) {
        let textures = &mut binding_resources.textures;
        let texture_views = &mut binding_resources.texture_views;
        let set_index = c.offset().set();
        let slot_index = c.offset().slot();
        if !textures.contains_key(&set_index) {
            textures.insert(set_index, HashMap::new());
            texture_views.insert(set_index, HashMap::new());
        }
        let tset = binding_resources.textures.get_mut(&set_index).unwrap();
        let tviewset = binding_resources.texture_views.get_mut(&set_index).unwrap();
        if !tset.contains_key(&slot_index) {
            let texture = device.create_texture(&self.descriptor());
            let texture_view = texture.create_view(&self.view_descriptor());
            tset.insert(slot_index, texture);
            tviewset.insert(slot_index, texture_view);
        } else {
            let curtex = tset.get_mut(&slot_index).unwrap();
            if curtex.width() != self.width() || curtex.height() != self.height() {
                let newtex = device.create_texture(&self.descriptor());
                let newview = newtex.create_view(&self.view_descriptor());
                tviewset.insert(slot_index, newview);
                *curtex = newtex;
            }
        }
        let tex = tset.get_mut(&slot_index).unwrap();
        queue.write_texture(
            TexelCopyTextureInfo {
                texture: &tex,
                mip_level: 0,
                origin: Origin3d { x: 0, y: 0, z: 0 },
                aspect: TextureAspect::All,
            },
            &self.as_bytevec(),
            TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(self.bytes_per_block() * self.width()),
                rows_per_image: Some(self.height()),
            },
            self.extent(),
        );
    }
}

fn describe_skybox(skybox: &RgbaSkybox) -> TextureDescriptor {
    TextureDescriptor {
        label: None,
        size: Extent3d {
            width: skybox.width(),
            height: skybox.height(),
            depth_or_array_layers: 6,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: TextureDimension::D2,
        format: TextureFormat::Rgba8UnormSrgb,
        usage: TextureUsages::COPY_DST | TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
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

#[derive(Writable)]
struct Camera {
    width: f32,
    height: f32,
    frame: Matrix3<f32>,
    frame_inv: Matrix3<f32>,
    centre: Vector3<f32>,
    yfov: f32,
}

// impl Writable for Camera {
//     fn write_at_cursor(
//         &self,
//         c: Cursor,
//         device: &wgpu::Device,
//         queue: &wgpu::Queue,
//         binding_resources: &mut BindingResources,
//     ) {
//         let width_cursor = c.navigate_field(0).unwrap();
//         let height_cursor = c.navigate_field(1).unwrap();
//         let frame_cursor = c.navigate_field(2).unwrap();
//         let frame_inv_cursor = c.navigate_field(3).unwrap();
//         let centre_cursor = c.navigate_field(4).unwrap();
//         let yfov_cursor = c.navigate_field(5).unwrap();
//         self.width.write_at_cursor(width_cursor, device, queue, binding_resources);
//         self.height.write_at_cursor(height_cursor, device, queue, binding_resources);
//         self.frame.write_at_cursor(frame_cursor, device, queue, binding_resources);
//         self.frame_inv.write_at_cursor(frame_inv_cursor, device, queue, binding_resources);
//         self.centre.write_at_cursor(centre_cursor, device, queue, binding_resources);
//         self.yfov.write_at_cursor(yfov_cursor, device, queue, binding_resources);
//     }
// }

struct CameraController {
    q_state: ElementState,
    e_state: ElementState,
    w_state: ElementState,
    s_state: ElementState,
    a_state: ElementState,
    d_state: ElementState,
}

impl CameraController {
    // TODO: modify this to use the metric
    fn update_camera(&mut self, camera: &mut Camera, dt: Duration) {
        const ANGULAR_SPEED: f32 = 1f32;
        const LINEAR_SPEED: f32 = 8f32;
        let dt_seconds = dt.as_secs_f32();

        let mut linvel = Vector3::<f32>::zero();
        let z_linvel = LINEAR_SPEED * Vector3::unit_z();
        let x_linvel = LINEAR_SPEED * Vector3::unit_x();

        if self.w_state.is_pressed() {
            linvel += z_linvel;
        }
        if self.s_state.is_pressed() {
            linvel -= z_linvel;
        }
        if self.d_state.is_pressed() {
            linvel += x_linvel;
        }
        if self.a_state.is_pressed() {
            linvel -= x_linvel;
        }
        camera.centre += camera.frame * (dt_seconds * linvel);
        // parallel_transport_camera(donut, camera, linvel, dt_seconds);

        let mut rotvel = Vector3::<f32>::zero();
        let z_rotvel = ANGULAR_SPEED * Vector3::unit_z();

        if self.q_state.is_pressed() {
            rotvel -= z_rotvel;
        }
        if self.e_state.is_pressed() {
            rotvel += z_rotvel;
        }
        let axis = rotvel.normalize();
        if axis.is_finite() {
            camera.frame =
                camera.frame * Matrix3::from_axis_angle(axis, Rad(dt_seconds * rotvel.magnitude()));
        }
        camera.frame_inv = camera.frame.invert().unwrap();
    }
    fn process_window_event(&mut self, event: &winit::event::WindowEvent) {
        match event {
            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        physical_key: PhysicalKey::Code(code),
                        repeat: false,
                        state,
                        ..
                    },
                ..
            } => match code {
                // TODO: refactor this to have a single source of truth
                KeyCode::KeyQ => self.q_state = *state,
                KeyCode::KeyE => self.e_state = *state,
                KeyCode::KeyW => self.w_state = *state,
                KeyCode::KeyS => self.s_state = *state,
                KeyCode::KeyA => self.a_state = *state,
                KeyCode::KeyD => self.d_state = *state,
                _ => {}
            },
            _ => {}
        }
    }
    fn process_mouse_motion(&mut self, camera: &mut Camera, delta: &(f64, f64)) {
        const ANGULAR_SPEED: f32 = 0.001;
        let dx = delta.0 as f32;
        let dy = delta.1 as f32;
        let angle = ANGULAR_SPEED * (dx.powi(2) + dy.powi(2)).sqrt();
        let axis = Vector3 {
            x: -dy,
            y: dx,
            z: 0.0,
        }
        .normalize();
        camera.frame = camera.frame * Matrix3::from_axis_angle(axis, Rad(angle));
    }
}

struct ConstantBuffer<T>(T);
impl<T: Writable> Writable for ConstantBuffer<T> {
    fn write_at_cursor(
        &self,
        c: Cursor,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        binding_resources: &mut BindingResources,
    ) {
        let element_cursor = c.navigate_child().unwrap();
        self.0
            .write_at_cursor(element_cursor, device, queue, binding_resources);
    }
}

struct StructuredBuffer<T>(Vec<T>);

impl<T: Writable> Writable for StructuredBuffer<T> {
    fn write_at_cursor(
        &self,
        c: Cursor,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        binding_resources: &mut BindingResources,
    ) {
        let total_bytes_needed = (self.0.len())
            * c.type_layout()
                .element_type_layout()
                .stride(slang::ParameterCategory::Uniform);
        let data_buffer = binding_resources
            .buffers
            .get_mut(&c.offset().set())
            .unwrap()
            .get_mut(&c.offset().slot())
            .unwrap();
        if data_buffer.size() != total_bytes_needed.try_into().unwrap() {
            *data_buffer = device.create_buffer(&BufferDescriptor {
                label: None,
                size: total_bytes_needed.try_into().unwrap(),
                usage: data_buffer.usage(),
                mapped_at_creation: false,
            });
        }
        for i in 0..self.0.len() {
            let element_cursor = c.navigate_index(i as u32).unwrap();
            self.0[i as usize].write_at_cursor(element_cursor, device, queue, binding_resources);
        }
    }
}

struct TR3 {
    q: Vector3<f32>,
    v: Vector3<f32>,
}

#[derive(Debug, Writable)]
struct Hermite {
    pos: Vector3<f32>,
    normal: Vector3<f32>,
}

#[derive(Writable)]
struct SurfaceParams {
    support: f32,
    point_count: i32,
    point_data: StructuredBuffer<Hermite>,
}

struct DummySampler {}

impl Writable for DummySampler {
    fn write_at_cursor(
        &self,
        c: Cursor,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        binding_resources: &mut BindingResources,
    ) {
        let set_index = c.offset().set();
        let slot_index = c.offset().slot();
        if !binding_resources.samplers.contains_key(&set_index) {
            binding_resources.samplers.insert(set_index, HashMap::new());
        }
        let set = binding_resources.samplers.get_mut(&set_index).unwrap();
        if !set.contains_key(&slot_index) {
            set.insert(
                slot_index,
                device.create_sampler(&SamplerDescriptor::default()),
            );
        }
    }
}

#[derive(Writable)]
struct GraphicsGlobal {
    camera: ConstantBuffer<Camera>,
    surface: ConstantBuffer<SurfaceParams>,
    background: ConstantBuffer<RgbaSkybox>,
    background_sampler: ConstantBuffer<DummySampler>,
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
        binding_resources: &mut BindingResources,
    ) {
        let bind_group_ix = c.offset().set();
        let bind_slot_ix = c.offset().slot();
        let uniform_offset = c.offset().uniform();
        let buffer = binding_resources
            .buffers
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
        binding_resources: &mut BindingResources,
    ) {
        let bind_group_ix = c.offset().set();
        let bind_slot_ix = c.offset().slot();
        let uniform_offset = c.offset().uniform();
        let buffer = binding_resources
            .buffers
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
        binding_resources: &mut BindingResources,
    ) {
        let bind_group_ix = c.offset().set();
        let bind_slot_ix = c.offset().slot();
        let uniform_offset = c.offset().uniform();
        let buffer = binding_resources
            .buffers
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
        binding_resources: &mut BindingResources,
    ) {
        let bind_group_ix = c.offset().set();
        let bind_slot_ix = c.offset().slot();
        let uniform_offset = c.offset().uniform();
        let buffer = binding_resources
            .buffers
            .get(&bind_group_ix)
            .unwrap()
            .get(&bind_slot_ix)
            .unwrap();
        let data = self.to_bytes();
        queue.write_buffer(buffer, uniform_offset.try_into().unwrap(), &data);
    }
}

// impl Writable for Hermite {
//     fn write_at_cursor(
//         &self,
//         c: reflection::Cursor,
//         device: &wgpu::Device,
//         queue: &wgpu::Queue,
//         binding_resources: &mut BindingResources,
//     ) {
//         let pos_cursor = c.navigate_field(0).unwrap();
//         let normal_cursor = c.navigate_field(1).unwrap();
//         self.pos.write_at_cursor(pos_cursor, device, queue, buffers);
//         self.normal
//             .write_at_cursor(normal_cursor, device, queue, buffers);
//     }
// }

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
    binding_resources: BindingResources,
    bind_group_layouts: HashMap<usize, BindGroupLayout>,
    layout_entries: HashMap<usize, Vec<BindGroupLayoutEntry>>,
    graphics_global: GraphicsGlobal,
    program: slang::ComponentType,
    slang_global_session: slang::GlobalSession,
    slang_session: slang::Session,
    fixed_time: Instant,
    mouse_capture_mode: CursorGrabMode,
    cursor_is_visible: bool,
    camera_controller: CameraController,
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
        self.graphics_global.write_at_cursor(
            top_cursor,
            &self.device,
            &self.queue,
            &mut self.binding_resources,
        );

        // Make bind groups
        let bind_group_entries =
            bind_group_entries_from_layout(&self.layout_entries, &self.binding_resources);
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
            self.graphics_global.camera.0.height = new_size.height as f32;
            self.graphics_global.camera.0.width = new_size.width as f32;
            self.surface_config.width = new_size.width;
            self.surface_config.height = new_size.height;
            self.surface.configure(&self.device, &self.surface_config);
        }
    }
    fn toggle_mouse_capture(&mut self) {
        let new_mode = match self.mouse_capture_mode {
            CursorGrabMode::None => CursorGrabMode::Locked,
            CursorGrabMode::Confined => CursorGrabMode::None,
            CursorGrabMode::Locked => CursorGrabMode::None,
        };
        let fallback_mode = match self.mouse_capture_mode {
            CursorGrabMode::None => CursorGrabMode::Confined,
            CursorGrabMode::Confined => CursorGrabMode::None,
            CursorGrabMode::Locked => CursorGrabMode::None,
        };
        let visibility = match new_mode {
            CursorGrabMode::None => true,
            CursorGrabMode::Confined => false,
            CursorGrabMode::Locked => false,
        };
        if let Err(_) = self.window.set_cursor_grab(new_mode) {
            self.window.set_cursor_grab(fallback_mode).unwrap();
        }
        self.window.set_cursor_visible(visibility);

        self.mouse_capture_mode = new_mode;
        self.cursor_is_visible = visibility;
    }
    fn window_input(&mut self, event: &WindowEvent) {
        // probably the camera controller should not have to know what the window event is
        // we're passing too much in
        // TODO: refactor this
        self.camera_controller.process_window_event(event);
        match event {
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Left,
                ..
            } => self.toggle_mouse_capture(),
            _ => {}
        }
    }
    fn device_input(&mut self, event: &DeviceEvent) {
        match self.mouse_capture_mode {
            CursorGrabMode::Confined | CursorGrabMode::Locked => {
                if let DeviceEvent::MouseMotion { delta } = event {
                    self.camera_controller
                        .process_mouse_motion(&mut self.graphics_global.camera.0, delta);
                }
            }
            _ => {}
        }
    }
    fn update_logic_state(&mut self, new_time: Instant) {
        let dt = new_time.duration_since(self.fixed_time);
        self.camera_controller
            .update_camera(&mut self.graphics_global.camera.0, dt);

        // Write all deferrable logic (not rendering) changes.
        // Maybe I should have wrapper logic just to set a bit telling me whether
        // a change needs to be written?
        self.fixed_time = new_time;
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
                let new_time = Instant::now();
                app.update_logic_state(new_time);
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
            yfov: PI / 3.0,
        };

        let point_data = StructuredBuffer(circle(9));
        let surface_params = SurfaceParams {
            support: 1.0,
            point_count: point_data.0.len().try_into().unwrap(),
            point_data,
        };

        let background = RgbaSkybox::load_from_path(Path::new("textures/bg1")).unwrap();
        let background_sampler = DummySampler {};

        let graphics_global = GraphicsGlobal {
            camera: ConstantBuffer(camera),
            surface: ConstantBuffer(surface_params),
            background: ConstantBuffer(background),
            background_sampler: ConstantBuffer(background_sampler),
        };

        let fixed_time = Instant::now();
        let mouse_capture_mode = CursorGrabMode::None;
        let cursor_is_visible = true;
        let camera_controller = CameraController {
            q_state: ElementState::Released,
            e_state: ElementState::Released,
            w_state: ElementState::Released,
            s_state: ElementState::Released,
            a_state: ElementState::Released,
            d_state: ElementState::Released,
        };

        let binding_resources = BindingResources {
            buffers,
            textures: HashMap::new(),
            texture_views: HashMap::new(),
            samplers: HashMap::new(),
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
            binding_resources,
            layout_entries,
            graphics_global,
            program,
            slang_global_session: global_session,
            slang_session: session,
            fixed_time,
            mouse_capture_mode,
            cursor_is_visible,
            camera_controller,
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
                e => app.window_input(&e),
            },
        }
    }

    fn device_event(
        &mut self,
        event_loop: &winit::event_loop::ActiveEventLoop,
        device_id: winit::event::DeviceId,
        event: winit::event::DeviceEvent,
    ) {
        match self {
            AppState::Uninitialized() => {}
            AppState::Initialized(app) => app.device_input(&event),
        }
    }
}
