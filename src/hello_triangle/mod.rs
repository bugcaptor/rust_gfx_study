use std::{borrow::Cow, mem};
use std::sync::Arc;
use web_time::{Duration, Instant};
use winit::{
    event::{Event, WindowEvent},
    event_loop::{ControlFlow, EventLoop, EventLoopWindowTarget},
    window::Window,
};
mod frame_counter;
use frame_counter::FrameCounter;

struct EventLoopWrapper {
    event_loop: EventLoop<()>,
    window: Arc<Window>,
}

impl EventLoopWrapper {
    pub fn new() -> Self {
        let event_loop = EventLoop::new().unwrap();
        #[allow(unused_mut)]
        let mut builder = winit::window::WindowBuilder::new();
        #[cfg(target_arch = "wasm32")]
        {
            use wasm_bindgen::JsCast;
            use winit::platform::web::WindowBuilderExtWebSys;
            let canvas = web_sys::window()
                .unwrap()
                .document()
                .unwrap()
                .get_element_by_id("canvas")
                .unwrap()
                .dyn_into::<web_sys::HtmlCanvasElement>()
                .unwrap();
            builder = builder.with_canvas(Some(canvas));
        }
        let window = Arc::new(builder.build(&event_loop).unwrap());

        Self {
            event_loop,
            window,
        }
    }    
}

struct Framework<'a> {
    frame_counter: FrameCounter,
    surface: wgpu::Surface<'a>,
    adapter: wgpu::Adapter,
    device: wgpu::Device,
    queue: wgpu::Queue,
    shader: wgpu::ShaderModule,
    config: wgpu::SurfaceConfiguration,
    instance: wgpu::Instance,
    render_pipeline: wgpu::RenderPipeline,
    pipeline_layout: wgpu::PipelineLayout,
}

impl Framework<'_> {
    pub async fn new(event_loop_wrapper: &EventLoopWrapper) -> Self {
        let mut size = event_loop_wrapper.window.inner_size();
        size.width = size.width.max(1);
        size.height = size.height.max(1);

        let instance = wgpu::Instance::default();

        let surface = instance.create_surface(Arc::clone(&event_loop_wrapper.window)).unwrap();

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::default(),
                force_fallback_adapter: false,
                // Request an adapter which can render to our surface
                compatible_surface: Some(&surface),
            })
            .await
            .expect("Failed to find an appropriate adapter");

        // Create the logical device and command queue
        let (device, queue) = adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    label: None,
                    required_features: wgpu::Features::empty(),
                    // Make sure we use the texture resolution limits from the adapter, so we can support images the size of the swapchain.
                    required_limits: wgpu::Limits::downlevel_webgl2_defaults()
                        .using_resolution(adapter.limits()),
                },
                None,
            )
            .await
            .expect("Failed to create device");

        // Load the shaders from disk
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: None,
            source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(include_str!("shader.wgsl"))),
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[],
            push_constant_ranges: &[],
        });

        let swapchain_capabilities = surface.get_capabilities(&adapter);
        let swapchain_format = swapchain_capabilities.formats[0];

        let render_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: None,
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_main",
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: "fs_main",
                compilation_options: Default::default(),
                targets: &[Some(swapchain_format.into())],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
        });

        let config = surface
            .get_default_config(&adapter, size.width, size.height)
            .unwrap();
        surface.configure(&device, &config);

        //let window = std::mem::take(&mut self.window);

        Self {
            frame_counter: FrameCounter::new(),
            surface,
            adapter,
            device,
            queue,
            shader,
            config,
            instance,
            render_pipeline,
            pipeline_layout,
        }
    }
    pub async fn run_loop(&mut self, event_loop_wrapper: EventLoopWrapper) {

        const FPS: u64 = 30;
        const FRAME_DURATION: Duration = Duration::from_millis(1000 / FPS); 
        let mut last_frame_time = Instant::now();

        event_loop_wrapper.event_loop.run(move |event: Event<()>, target: &EventLoopWindowTarget<()>| {
                if let Event::WindowEvent {
                    window_id: _,
                    event,
                } = event
                {
                    match event {
                        WindowEvent::Resized(new_size) => {
                            // Reconfigure the surface with the new size
                            self.config.width = new_size.width.max(1);
                            self.config.height = new_size.height.max(1);
                            self.surface.configure(&self.device, &self.config);
                            // On macos the window needs to be redrawn manually after resizing
                            event_loop_wrapper.window.request_redraw();
                        }
                        WindowEvent::RedrawRequested => {

                            // set to window title.
                            let title = format!("FPS: {:.1}", self.frame_counter.get_last_fps());
                            event_loop_wrapper.window.set_title(title.as_str());

                            // Calculate when the next frame should be
                            let now = Instant::now();
                            let duration = now.duration_since(last_frame_time);
                            if duration >= FRAME_DURATION {
                                last_frame_time = now;
                                self.render_frame();
                            }

                            event_loop_wrapper.window.request_redraw();
                        }
                        WindowEvent::CloseRequested => target.exit(),
                        _ => {}
                    };
                }
            })
            .unwrap();
    }
    fn render_frame(&mut self) {
        let frame = self.surface
                        .get_current_texture()
                        .expect("Failed to acquire next swap chain texture");
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder =
            self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: None,
            });
        {
            let mut rpass =
                encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: None,
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::GREEN),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                });
            rpass.set_pipeline(&self.render_pipeline);
            rpass.draw(0..3, 0..1);
        }

        self.queue.submit(Some(encoder.finish()));
        frame.present();

        self.frame_counter.update();
    }
}

pub async fn main() {
    let event_loop_wrapper = EventLoopWrapper::new();
    let mut framework = Framework::new(&event_loop_wrapper).await;

    #[cfg(not(target_arch = "wasm32"))]
    {
        env_logger::init();
        pollster::block_on(framework.run_loop(event_loop_wrapper));
    }
    #[cfg(target_arch = "wasm32")]
    {
        std::panic::set_hook(Box::new(console_error_panic_hook::hook));
        console_log::init().expect("could not initialize logger");
        wasm_bindgen_futures::spawn_local(framework.run());
    }
}
