pub mod reflection;

use std::{cell::RefCell, collections::HashMap, marker::PhantomPinned, pin::Pin, sync::Arc};

use encase::{internal::WriteInto, ShaderType, UniformBuffer};
use self_cell::self_cell;
use wgpu::*;
use winit::{
    application::ApplicationHandler,
    dpi::PhysicalSize,
    event::WindowEvent,
    event_loop::{self, ActiveEventLoop, EventLoop},
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

self_cell!(
    struct Bundle {
        owner: Vec<BindGroupLayoutEntry>,

        #[covariant]
        dependent: BindGroupLayoutDescriptor,
    }
    impl {Debug}
);

fn build_bundle() -> Bundle {
    let owner = vec![];
    Bundle::new(owner, |x| BindGroupLayoutDescriptor {
        label: None,
        entries: x.as_slice(),
    })
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
}

impl<'a> App<'a> {
    fn render(&self) -> Result<(), SurfaceError> {
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

        let bg0ld = BindGroupLayoutDescriptor {
            label: Some("bg0l"),
            entries: &[
                BindGroupLayoutEntry {
                    binding: 0,
                    visibility: ShaderStages::all(),
                    ty: BindingType::Buffer {
                        ty: BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                BindGroupLayoutEntry {
                    binding: 1,
                    visibility: ShaderStages::all(),
                    ty: BindingType::Buffer {
                        ty: BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                BindGroupLayoutEntry {
                    binding: 2,
                    visibility: ShaderStages::all(),
                    ty: BindingType::Buffer {
                        ty: BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        };
        let bg0l = device.create_bind_group_layout(&bg0ld);


        let render_pipeline = todo!();

        *self = AppState::Initialized(App {
            window,
            surface,
            size,
            surface_config,
            device,
            queue,
            render_pipeline,
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
